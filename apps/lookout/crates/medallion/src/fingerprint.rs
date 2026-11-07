use std::fmt::{self, Display};
use std::sync::Arc;

use futures::TryStreamExt;
use object_store::path::Path as ObjectPath;
use object_store::{ObjectMeta, ObjectStore};
use parquet::arrow::async_reader::{AsyncFileReader, ParquetObjectReader};
use parquet::file::metadata::ParquetMetaData;
use uuid::Uuid;

use crate::layer::LayerKind;
use crate::path::Dataset;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fingerprint(String);

impl Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FingerprintError {
    #[error("{layer} {dataset} has never been written, so it has no fingerprint")]
    Unwritten {
        layer: &'static str,
        dataset: &'static str,
    },
    #[error("object store error: {0}")]
    Store(#[from] object_store::Error),
    #[error("reading the parquet footer of {location}: {source}")]
    Footer {
        location: String,
        #[source]
        source: parquet::errors::ParquetError,
    },
}

const PARQUET: &str = "parquet";
const NAMESPACE: &[u8] = b"DATASET_FINGERPRINT";

impl<L: LayerKind> Dataset<L> {
    pub async fn fingerprint(&self) -> Result<Fingerprint, FingerprintError> {
        let store = self.root().object_store()?;
        fingerprint_in(store, &self.location())
            .await?
            .ok_or_else(|| FingerprintError::Unwritten {
                layer: self.layer(),
                dataset: self.name(),
            })
    }
}

pub async fn fingerprint_in(
    store: Arc<dyn ObjectStore>,
    location: &ObjectPath,
) -> Result<Option<Fingerprint>, FingerprintError> {
    let listed: Vec<ObjectMeta> = match store.list(Some(location)).try_collect().await {
        Err(object_store::Error::NotFound { .. }) => Vec::new(),
        listed => listed?,
    };
    let mut files: Vec<ObjectMeta> = listed
        .into_iter()
        .filter(|object| object.location.extension() == Some(PARQUET))
        .collect();
    if files.is_empty() {
        return Ok(None);
    }
    files.sort_by(|one, other| one.location.cmp(&other.location));
    let mut described = Vec::new();
    for file in &files {
        let within = file
            .location
            .prefix_match(location)
            .map(|parts| {
                parts
                    .map(|part| part.as_ref().to_string())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_else(|| file.location.to_string());
        described.extend_from_slice(within.as_bytes());
        described.push(0);
        described.extend_from_slice(
            description(metadata(Arc::clone(&store), file).await?.as_ref()).as_bytes(),
        );
    }
    Ok(Some(Fingerprint(
        Uuid::new_v5(&Uuid::new_v5(&Uuid::nil(), NAMESPACE), &described).to_string(),
    )))
}

async fn metadata(
    store: Arc<dyn ObjectStore>,
    file: &ObjectMeta,
) -> Result<Arc<ParquetMetaData>, FingerprintError> {
    ParquetObjectReader::new(store, file.location.clone())
        .with_file_size(file.size)
        .get_metadata(None)
        .await
        .map_err(|source| FingerprintError::Footer {
            location: file.location.to_string(),
            source,
        })
}

fn description(metadata: &ParquetMetaData) -> String {
    let columns = metadata
        .file_metadata()
        .schema_descr()
        .columns()
        .iter()
        .map(|column| {
            format!(
                "column {} {} {} {}\n",
                column.path().string(),
                column.physical_type(),
                column.max_def_level(),
                column.max_rep_level()
            )
        });
    let row_groups = metadata.row_groups().iter().flat_map(|row_group| {
        std::iter::once(format!("rows {}\n", row_group.num_rows())).chain(
            row_group.columns().iter().map(|chunk| {
                let statistics = chunk.statistics();
                format!(
                    "chunk {} {} {} {} {:?} {:?} {:?}\n",
                    chunk.column_path().string(),
                    chunk.compressed_size(),
                    chunk.uncompressed_size(),
                    chunk.num_values(),
                    statistics.and_then(|statistics| statistics.null_count_opt()),
                    statistics.and_then(|statistics| statistics.min_bytes_opt()),
                    statistics.and_then(|statistics| statistics.max_bytes_opt()),
                )
            }),
        )
    });
    columns.chain(row_groups).collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::{DateTime, NaiveDate, TimeZone, Utc};
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::dataset::DatasetSpec;
    use crate::derive::write_rows;
    use crate::layer::layers;
    use crate::path::Root;
    use crate::rows::{Dated, Row};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Reading {
        reading_id: String,
        #[serde(with = "chrono::serde::ts_milliseconds")]
        taken_at: DateTime<Utc>,
        level: f64,
    }

    impl Row for Reading {
        type Layer = layers::Silver;
        const DATASET: DatasetSpec<Self::Layer> = DatasetSpec::partitioned("reading", "taken_date");
        const INSTANTS: &'static [&'static str] = &["taken_at"];

        fn example() -> Self {
            Reading {
                reading_id: "example".to_string(),
                taken_at: DateTime::UNIX_EPOCH,
                level: 0.0,
            }
        }
    }

    impl Dated for Reading {
        fn partition_date(&self) -> NaiveDate {
            self.taken_at.date_naive()
        }
    }

    fn readings(days: u32, level: f64) -> Vec<Reading> {
        (1..=days)
            .map(|day| Reading {
                reading_id: format!("reading-{day}"),
                taken_at: Utc
                    .with_ymd_and_hms(2026, 7, 20 + day, 9, 0, 0)
                    .single()
                    .expect("an instant"),
                level,
            })
            .collect()
    }

    async fn written(dir: &Path, rows: &[Reading]) -> Root {
        let root = Root::new(dir);
        write_rows(&root, rows).await.expect("write the readings");
        root
    }

    async fn fingerprint_of(root: &Root) -> Fingerprint {
        root.rows_of::<Reading>()
            .fingerprint()
            .await
            .expect("a fingerprint")
    }

    #[tokio::test]
    async fn a_fingerprint_holds_while_the_dataset_is_untouched() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = written(tmp.path(), &readings(2, 1.0)).await;

        assert_eq!(fingerprint_of(&root).await, fingerprint_of(&root).await);
    }

    #[tokio::test]
    async fn rewriting_the_same_rows_keeps_the_fingerprint() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = written(tmp.path(), &readings(2, 1.0)).await;
        let before = fingerprint_of(&root).await;

        write_rows(&root, &readings(2, 1.0))
            .await
            .expect("rewrite the readings");

        assert_eq!(fingerprint_of(&root).await, before);
    }

    #[tokio::test]
    async fn changed_rows_or_another_partition_change_the_fingerprint() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = written(tmp.path(), &readings(2, 1.0)).await;
        let before = fingerprint_of(&root).await;

        write_rows(&root, &readings(2, 2.0))
            .await
            .expect("change the readings");
        let changed = fingerprint_of(&root).await;
        write_rows(&root, &readings(3, 2.0))
            .await
            .expect("add a day");
        let added = fingerprint_of(&root).await;

        assert_ne!(changed, before);
        assert_ne!(added, changed);
    }

    #[tokio::test]
    async fn two_stores_holding_the_same_rows_agree() {
        let one = tempfile::tempdir().expect("a temporary directory");
        let other = tempfile::tempdir().expect("a temporary directory");

        let first = fingerprint_of(&written(one.path(), &readings(2, 1.0)).await).await;
        let second = fingerprint_of(&written(other.path(), &readings(2, 1.0)).await).await;

        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn a_file_that_is_not_parquet_does_not_count() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = written(tmp.path(), &readings(2, 1.0)).await;
        let before = fingerprint_of(&root).await;

        std::fs::write(root.rows_of::<Reading>().dir().join("notes.txt"), "a note")
            .expect("a stray file");

        assert_eq!(fingerprint_of(&root).await, before);
    }

    #[tokio::test]
    async fn a_dataset_never_written_is_refused() {
        let tmp = tempfile::tempdir().expect("a temporary directory");

        let refused = Root::new(tmp.path())
            .rows_of::<Reading>()
            .fingerprint()
            .await;

        assert!(
            matches!(refused, Err(FingerprintError::Unwritten { .. })),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn the_same_files_in_another_object_store_give_the_same_fingerprint() {
        use object_store::memory::InMemory;
        use object_store::{ObjectStore, PutPayload};

        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = written(tmp.path(), &readings(2, 1.0)).await;
        let dataset = root.rows_of::<Reading>();
        let local = root.object_store().expect("the root's store");
        let memory = InMemory::new();
        let files: Vec<ObjectMeta> = local
            .list(Some(&dataset.location()))
            .try_collect()
            .await
            .expect("the dataset's files");
        for file in files {
            let contents = local
                .get(&file.location)
                .await
                .expect("a file")
                .bytes()
                .await
                .expect("its bytes");
            memory
                .put(&file.location, PutPayload::from(contents))
                .await
                .expect("a copy");
        }

        let in_memory = fingerprint_in(Arc::new(memory), &dataset.location())
            .await
            .expect("a fingerprint");

        assert_eq!(in_memory, Some(fingerprint_of(&root).await));
    }

    #[tokio::test]
    async fn the_writer_of_the_same_rows_does_not_change_the_fingerprint() {
        use object_store::memory::InMemory;
        use object_store::{ObjectStore, PutPayload};
        use parquet::arrow::ArrowWriter;
        use parquet::file::properties::WriterProperties;

        let location = ObjectPath::from("silver/reading");
        let written_by = |created_by: &str| {
            let batch = crate::rows::batch(&readings(2, 1.0)).expect("a batch");
            let properties = WriterProperties::builder()
                .set_created_by(created_by.to_string())
                .build();
            let mut file = Vec::new();
            let mut writer = ArrowWriter::try_new(&mut file, batch.schema(), Some(properties))
                .expect("a writer");
            writer.write(&batch).expect("the rows");
            writer.close().expect("a footer");
            file
        };
        let mut fingerprints = Vec::new();
        for created_by in ["one writer", "another writer, version 2"] {
            let store = InMemory::new();
            store
                .put(
                    &location
                        .child("taken_date=2026-07-21")
                        .child("part-0.parquet"),
                    PutPayload::from(written_by(created_by)),
                )
                .await
                .expect("a file");
            fingerprints.push(
                fingerprint_in(Arc::new(store), &location)
                    .await
                    .expect("a fingerprint"),
            );
        }

        assert_eq!(fingerprints[0], fingerprints[1]);
    }
}
