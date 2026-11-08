use std::sync::Arc;

use arrow::array::RecordBatch;
use object_store::ObjectStore;
use object_store::path::Path as ObjectPath;
use parquet::arrow::AsyncArrowWriter;
use parquet::arrow::async_writer::ParquetObjectWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;

const ZSTD_LEVEL: i32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("creating the store's root at {path}: {source}")]
    Root {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("object store error: {0}")]
    Store(#[from] object_store::Error),
    #[error("parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    #[error("no batches to write")]
    Empty,
    #[error("{path} already holds a capture, which an append must not replace")]
    Exists { path: String },
}

pub(crate) async fn write_batches(
    store: Arc<dyn ObjectStore>,
    location: ObjectPath,
    batches: &[RecordBatch],
) -> Result<(), WriteError> {
    let Some(first) = batches.first() else {
        return Err(WriteError::Empty);
    };

    let mut writer = writer_at(store, location, first.schema())?;
    for batch in batches {
        writer.write(batch).await?;
    }
    writer.close().await?;
    Ok(())
}

pub(crate) async fn refuse_existing(
    store: &dyn ObjectStore,
    location: &ObjectPath,
) -> Result<(), WriteError> {
    match store.head(location).await {
        Ok(_) => Err(WriteError::Exists {
            path: location.to_string(),
        }),
        Err(object_store::Error::NotFound { .. }) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn writer_at(
    store: Arc<dyn ObjectStore>,
    location: ObjectPath,
    schema: arrow::datatypes::SchemaRef,
) -> Result<AsyncArrowWriter<ParquetObjectWriter>, WriteError> {
    let object_writer = ParquetObjectWriter::new(store, location);
    let properties = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::try_new(ZSTD_LEVEL)?))
        .build();
    Ok(AsyncArrowWriter::try_new(
        object_writer,
        schema,
        Some(properties),
    )?)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::dataset::DatasetSpec;
    use crate::layer::layers;
    use crate::path::Root;

    const SENSOR_READING: DatasetSpec<layers::Bronze> =
        DatasetSpec::partitioned("sensor_reading", "ingested_date");
    use arrow::array::{Int64Array, RecordBatch, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};
    use chrono::{TimeZone, Utc};

    use super::*;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    fn batch(ids: Vec<i64>, names: Vec<&str>) -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("name", DataType::Utf8, false),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int64Array::from(ids)),
                Arc::new(StringArray::from(names)),
            ],
        )
        .unwrap()
    }

    fn destination(
        root: &Root,
        name: &str,
    ) -> (Arc<dyn ObjectStore>, ObjectPath, std::path::PathBuf) {
        (
            root.object_store().unwrap(),
            ObjectPath::from(name),
            root.path().join(name),
        )
    }

    fn rows_in(path: &std::path::Path) -> usize {
        let file = std::fs::File::open(path).unwrap();
        ParquetRecordBatchReaderBuilder::try_new(file)
            .unwrap()
            .build()
            .unwrap()
            .map(|b| b.unwrap().num_rows())
            .sum()
    }

    #[tokio::test]
    async fn writing_creates_the_partition_directories_and_a_readable_parquet_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (store, location, path) =
            destination(&Root::new(tmp.path()), "bronze/a=1/b=2/part-0.parquet");

        write_batches(store, location, &[batch(vec![1, 2], vec!["a", "b"])])
            .await
            .unwrap();

        assert!(path.exists(), "{} should exist", path.display());
        assert_eq!(rows_in(&path), 2);
    }

    #[tokio::test]
    async fn every_column_of_every_row_group_is_compressed_with_zstd() {
        let tmp = tempfile::tempdir().unwrap();
        let (store, location, path) = destination(&Root::new(tmp.path()), "part-0.parquet");

        write_batches(store, location, &[batch(vec![1, 2], vec!["a", "b"])])
            .await
            .unwrap();

        let file = std::fs::File::open(&path).unwrap();
        let reader = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
        let columns: Vec<_> = reader
            .metadata()
            .row_groups()
            .iter()
            .flat_map(|group| group.columns())
            .collect();
        assert!(!columns.is_empty());
        for column in columns {
            assert!(
                matches!(column.compression(), Compression::ZSTD(_)),
                "{} is {}",
                column.column_path(),
                column.compression()
            );
        }
    }

    #[tokio::test]
    async fn multiple_batches_are_written_into_the_one_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (store, location, path) = destination(&Root::new(tmp.path()), "part-0.parquet");

        write_batches(
            store,
            location,
            &[batch(vec![1], vec!["a"]), batch(vec![2, 3], vec!["b", "c"])],
        )
        .await
        .unwrap();

        assert_eq!(rows_in(&path), 3);
    }

    #[tokio::test]
    async fn writing_no_batches_is_an_error_rather_than_an_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (store, location, path) = destination(&Root::new(tmp.path()), "part-0.parquet");

        let err = write_batches(store, location, &[]).await.unwrap_err();

        assert!(matches!(err, WriteError::Empty), "unexpected error: {err}");
        assert!(!path.exists(), "no file should have been created");
    }

    #[tokio::test]
    async fn a_failed_write_leaves_no_file_at_the_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let (store, location, path) = destination(&Root::new(tmp.path()), "part-0.parquet");
        let mismatched = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)])),
            vec![Arc::new(Int64Array::from(vec![9]))],
        )
        .unwrap();

        let err = write_batches(store, location, &[batch(vec![1], vec!["a"]), mismatched])
            .await
            .unwrap_err();

        assert!(matches!(err, WriteError::Parquet(_)), "unexpected: {err}");
        assert!(
            !path.exists(),
            "a partial file was left at {}",
            path.display()
        );
    }

    #[tokio::test]
    async fn appending_where_a_capture_already_sits_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let at = Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 30).unwrap();
        let dataset = Root::new(tmp.path())
            .dataset(SENSOR_READING)
            .on_date(at.date_naive())
            .unwrap();
        let path = dataset
            .append(at, &[batch(vec![1], vec!["a"])])
            .await
            .unwrap();

        let err = dataset
            .append(at, &[batch(vec![2], vec!["b"])])
            .await
            .unwrap_err();

        assert!(
            matches!(err, WriteError::Exists { .. }),
            "unexpected: {err}"
        );
        assert_eq!(rows_in(&path), 1, "the first capture must survive");
    }

    #[tokio::test]
    async fn appending_into_a_root_not_yet_made_makes_it() {
        let tmp = tempfile::tempdir().unwrap();
        let at = Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 30).unwrap();
        let dataset = Root::new(tmp.path().join("not-yet-made"))
            .dataset(SENSOR_READING)
            .on_date(at.date_naive())
            .unwrap();

        let path = dataset
            .append(at, &[batch(vec![1], vec!["a"])])
            .await
            .unwrap();

        assert_eq!(rows_in(&path), 1);
    }
}
