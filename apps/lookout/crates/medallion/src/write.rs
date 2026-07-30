//! Writing a batch of rows into the store as one parquet file.

use std::path::Path;

use arrow::array::RecordBatch;
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectPath;
use parquet::arrow::async_writer::ParquetObjectWriter;
use parquet::arrow::AsyncArrowWriter;

/// Failure writing a parquet file into the store.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("{path} is not an absolute path into the store")]
    Path {
        path: String,
        #[source]
        source: object_store::path::Error,
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

/// Write `batches` to `path` as a single parquet file, taking the schema from the first.
///
/// The file appears at `path` only once fully written: the write goes through
/// [`LocalFileSystem`], which stages to a temporary sibling and renames on completion, so
/// an interrupted write leaves nothing a reader can list or open.
pub(crate) async fn write_batches(path: &Path, batches: &[RecordBatch]) -> Result<(), WriteError> {
    let Some(first) = batches.first() else {
        return Err(WriteError::Empty);
    };

    let mut writer = writer_at(path, first.schema())?;
    for batch in batches {
        writer.write(batch).await?;
    }
    writer.close().await?;
    Ok(())
}

/// A parquet writer onto `path`, through the store's write path: [`LocalFileSystem`]
/// stages to a temporary sibling and renames on completion, so an interrupted write leaves
/// nothing a reader can list or open.
pub(crate) fn writer_at(
    path: &Path,
    schema: arrow::datatypes::SchemaRef,
) -> Result<AsyncArrowWriter<ParquetObjectWriter>, WriteError> {
    let location = ObjectPath::from_absolute_path(path).map_err(|source| WriteError::Path {
        path: path.display().to_string(),
        source,
    })?;
    let object_writer =
        ParquetObjectWriter::new(std::sync::Arc::new(LocalFileSystem::new()), location);
    Ok(AsyncArrowWriter::try_new(object_writer, schema, None)?)
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
        let path = Root::new(tmp.path())
            .dataset(SENSOR_READING)
            .partition("sensor", "gps")
            .unwrap()
            .batch_file(Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 30).unwrap());

        write_batches(&path, &[batch(vec![1, 2], vec!["a", "b"])])
            .await
            .unwrap();

        assert!(path.exists(), "{} should exist", path.display());
        assert_eq!(rows_in(&path), 2);
    }

    #[tokio::test]
    async fn multiple_batches_are_written_into_the_one_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = Root::new(tmp.path())
            .dataset(SENSOR_READING)
            .partition_file();

        write_batches(
            &path,
            &[batch(vec![1], vec!["a"]), batch(vec![2, 3], vec!["b", "c"])],
        )
        .await
        .unwrap();

        assert_eq!(rows_in(&path), 3);
    }

    #[tokio::test]
    async fn writing_no_batches_is_an_error_rather_than_an_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = Root::new(tmp.path())
            .dataset(SENSOR_READING)
            .partition_file();

        let err = write_batches(&path, &[]).await.unwrap_err();

        assert!(matches!(err, WriteError::Empty), "unexpected error: {err}");
        assert!(!path.exists(), "no file should have been created");
    }

    #[tokio::test]
    async fn a_failed_write_leaves_no_file_at_the_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let path = Root::new(tmp.path())
            .dataset(SENSOR_READING)
            .partition_file();
        // Batches with differing schemas: the first is accepted, the second fails mid-write.
        let mismatched = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)])),
            vec![Arc::new(Int64Array::from(vec![9]))],
        )
        .unwrap();

        let err = write_batches(&path, &[batch(vec![1], vec!["a"]), mismatched])
            .await
            .unwrap_err();

        assert!(matches!(err, WriteError::Parquet(_)), "unexpected: {err}");
        assert!(
            !path.exists(),
            "a partial file was left at {}",
            path.display()
        );
    }

    /// These layers are immutable, so a second append at the same instant must fail rather
    /// than replace what the first wrote. Two writes close together — a drain writing its
    /// batches, a backfill replaying an archive — is the case this catches.
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
    async fn a_relative_destination_is_rejected() {
        let err = write_batches(
            std::path::Path::new("relative/part-0.parquet"),
            &[batch(vec![1], vec!["a"])],
        )
        .await
        .unwrap_err();

        assert!(matches!(err, WriteError::Path { .. }), "unexpected: {err}");
    }
}
