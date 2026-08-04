//! What a store currently holds, read from the files themselves.
//!
//! A summary answers "is this dataset there, and how much of it" without reading any rows:
//! row counts come from each parquet file's own footer, so the cost is a seek per file
//! rather than a scan. Nothing here interprets a dataset's columns, which is what lets one
//! summary cover every dataset — including the ones whose partitions hold different schemas
//! and so cannot be read as a single table.
//!
//! Absence is a result, not an error: a dataset nothing has written yet is summarised as
//! holding nothing, so a reader sees the gaps as well as the contents.

use std::path::Path;

use parquet::errors::ParquetError;
use parquet::file::reader::{FileReader, SerializedFileReader};

use crate::dataset::DatasetInfo;
use crate::layer::Layer;
use crate::path::Root;

/// The extension of the files whose rows can be counted; anything else contributes its
/// bytes but no rows.
const PARQUET: &str = "parquet";

/// A failure reading what the store holds.
#[derive(Debug, thiserror::Error)]
pub enum SummaryError {
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("reading the row count of {path}: {source}")]
    Parquet {
        path: String,
        #[source]
        source: ParquetError,
    },
}

/// How much data some part of the store holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Contents {
    pub files: usize,
    /// Rows across the parquet files; files of any other format count none.
    pub rows: u64,
    pub bytes: u64,
}

impl Contents {
    /// Count `other` into this, for a total over parts summarised separately.
    pub fn add(&mut self, other: Contents) {
        self.files += other.files;
        self.rows += other.rows;
        self.bytes += other.bytes;
    }

    /// Whether anything has been written here at all.
    pub fn is_empty(self) -> bool {
        self.files == 0
    }
}

/// What one dataset holds, and how it is spread over its partitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetSummary {
    pub layer: Layer,
    pub name: &'static str,
    /// One entry per value of the dataset's own partition key, in the order the key sorts,
    /// and empty for an unpartitioned dataset or one holding nothing.
    pub partitions: Vec<PartitionSummary>,
    pub contents: Contents,
}

/// What one partition of a dataset holds. A dataset partitioned more deeply than its own
/// key — an extract keeping an upstream's layout below it — is still summarised per value
/// of its own key, with everything below that value counted into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionSummary {
    pub value: String,
    pub contents: Contents,
}

/// What one gold artefact holds: a file in a format of its own, kept per run that produced
/// it, so every version stands beside the last rather than replacing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtefactSummary {
    pub artifact: String,
    /// One entry per run, oldest first — the versions sort chronologically.
    pub versions: Vec<VersionSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionSummary {
    pub version: String,
    pub contents: Contents,
}

/// What `dataset` holds in `root`.
pub fn dataset(root: &Root, dataset: DatasetInfo) -> Result<DatasetSummary, SummaryError> {
    let dir = root.path().join(dataset.layer.as_str()).join(dataset.name);
    let mut summary = DatasetSummary {
        layer: dataset.layer,
        name: dataset.name,
        partitions: Vec::new(),
        contents: Contents::default(),
    };

    if dataset.partition_key.is_none() {
        summary.contents = contents_of(&dir)?;
        return Ok(summary);
    }
    for partition in sorted_dirs(&dir)? {
        let contents = contents_of(&partition.path())?;
        summary.contents.add(contents);
        summary.partitions.push(PartitionSummary {
            value: partition_value(&partition.file_name().to_string_lossy()),
            contents,
        });
    }
    summary
        .partitions
        .retain(|partition| !partition.contents.is_empty());
    Ok(summary)
}

/// What gold artefacts `root` holds, by artefact and then by the run that produced each
/// version.
pub fn artefacts(root: &Root) -> Result<Vec<ArtefactSummary>, SummaryError> {
    let mut artefacts = Vec::new();
    for artifact in sorted_dirs(&root.path().join(Layer::Gold.as_str()))? {
        let mut versions = Vec::new();
        for version in sorted_dirs(&artifact.path())? {
            versions.push(VersionSummary {
                version: partition_value(&version.file_name().to_string_lossy()),
                contents: contents_of(&version.path())?,
            });
        }
        versions.retain(|version| !version.contents.is_empty());
        if !versions.is_empty() {
            artefacts.push(ArtefactSummary {
                artifact: partition_value(&artifact.file_name().to_string_lossy()),
                versions,
            });
        }
    }
    Ok(artefacts)
}

/// The value half of a `key=value` directory name, or the whole name if it is not one —
/// a summary reports what is on disk rather than refusing to describe it.
fn partition_value(dir_name: &str) -> String {
    dir_name
        .split_once('=')
        .map_or(dir_name, |(_, value)| value)
        .to_string()
}

/// The directories directly below `dir`, in name order; none at all if `dir` does not
/// exist, which is how an absent dataset summarises as holding nothing.
fn sorted_dirs(dir: &Path) -> Result<Vec<std::fs::DirEntry>, SummaryError> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| SummaryError::Io {
            path: dir.display().to_string(),
            source,
        })?;
        if entry.path().is_dir() {
            dirs.push(entry);
        }
    }
    dirs.sort_by_key(std::fs::DirEntry::file_name);
    Ok(dirs)
}

/// Everything below `dir`, at any depth.
fn contents_of(dir: &Path) -> Result<Contents, SummaryError> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Contents::default());
    };
    let mut contents = Contents::default();
    for entry in entries {
        let path = entry
            .map_err(|source| SummaryError::Io {
                path: dir.display().to_string(),
                source,
            })?
            .path();
        if path.is_dir() {
            contents.add(contents_of(&path)?);
        } else {
            contents.add(file_contents(&path)?);
        }
    }
    Ok(contents)
}

/// One file's size, and its rows if it is one the store counts rows in.
fn file_contents(path: &Path) -> Result<Contents, SummaryError> {
    let file = std::fs::File::open(path).map_err(|source| SummaryError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let bytes = file
        .metadata()
        .map_err(|source| SummaryError::Io {
            path: path.display().to_string(),
            source,
        })?
        .len();
    let rows = match path.extension().is_some_and(|kind| kind == PARQUET) {
        true => rows_in(file, path)?,
        false => 0,
    };
    Ok(Contents {
        files: 1,
        rows,
        bytes,
    })
}

/// The rows a parquet file declares in its own footer, so counting them reads no data.
fn rows_in(file: std::fs::File, path: &Path) -> Result<u64, SummaryError> {
    let reader = SerializedFileReader::new(file).map_err(|source| SummaryError::Parquet {
        path: path.display().to_string(),
        source,
    })?;
    Ok(reader.metadata().file_metadata().num_rows().max(0) as u64)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::array::{Int64Array, RecordBatch};
    use arrow::datatypes::{DataType, Field, Schema};
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::dataset::DatasetSpec;
    use crate::layer::layers;

    const THING: DatasetSpec<layers::Bronze> = DatasetSpec::partitioned("thing", "kind");
    const WHOLE: DatasetSpec<layers::Bronze> = DatasetSpec::unpartitioned("whole");

    /// Append a file of `rows` rows to `dataset`, named for an instant of its own so two
    /// writes to one partition are two files.
    async fn write(dataset: crate::path::Dataset<layers::Bronze>, rows: i64) {
        let schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)]));
        let batch = RecordBatch::try_new(
            schema,
            vec![Arc::new(Int64Array::from((0..rows).collect::<Vec<_>>()))],
        )
        .unwrap();
        let at = Utc
            .with_ymd_and_hms(2026, 7, 26, 9, 0, rows as u32)
            .unwrap();
        dataset.append(at, &[batch]).await.unwrap();
    }

    #[tokio::test]
    async fn a_dataset_is_summarised_per_value_of_its_own_partition_key() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        write(root.dataset(THING).partition("kind", "b").unwrap(), 3).await;
        write(root.dataset(THING).partition("kind", "a").unwrap(), 2).await;
        write(root.dataset(THING).partition("kind", "a").unwrap(), 4).await;

        let summary = dataset(&root, THING.info()).unwrap();

        assert_eq!(summary.contents.rows, 9);
        assert_eq!(summary.contents.files, 3);
        assert!(summary.contents.bytes > 0);
        let partitions: Vec<(&str, u64, usize)> = summary
            .partitions
            .iter()
            .map(|partition| {
                (
                    partition.value.as_str(),
                    partition.contents.rows,
                    partition.contents.files,
                )
            })
            .collect();
        assert_eq!(partitions, vec![("a", 6, 2), ("b", 3, 1)]);
    }

    /// An unpartitioned dataset is one thing, so it is summarised as one and not broken
    /// down.
    #[tokio::test]
    async fn an_unpartitioned_dataset_has_no_partitions_to_report() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        write(root.dataset(WHOLE), 5).await;

        let summary = dataset(&root, WHOLE.info()).unwrap();

        assert!(summary.partitions.is_empty());
        assert_eq!(summary.contents.rows, 5);
    }

    /// A dataset nothing has written is reported as holding nothing rather than as a
    /// failure: what a store is missing is the point of asking.
    #[test]
    fn a_dataset_that_was_never_written_holds_nothing() {
        let tmp = tempfile::tempdir().unwrap();

        let summary = dataset(&Root::new(tmp.path()), THING.info()).unwrap();

        assert!(summary.contents.is_empty());
        assert_eq!(summary.contents, Contents::default());
        assert!(summary.partitions.is_empty());
    }

    /// A rebuild that produces nothing sweeps a partition and leaves its directory
    /// standing; an empty directory is not a partition the store holds.
    #[test]
    fn a_partition_swept_empty_is_not_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        std::fs::create_dir_all(root.dataset(THING).partition("kind", "a").unwrap().dir()).unwrap();

        let summary = dataset(&root, THING.info()).unwrap();

        assert!(summary.partitions.is_empty());
        assert!(summary.contents.is_empty());
    }

    /// Gold artefacts are not parquet, so they are summarised by what they weigh and which
    /// runs produced them, with no rows to count.
    #[test]
    fn gold_artefacts_are_summarised_by_artefact_and_run() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        for run in [
            Utc.with_ymd_and_hms(2026, 7, 26, 9, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 7, 27, 9, 0, 0).unwrap(),
        ] {
            let path = root
                .gold_artefact("crossings", run, "crossings.pointset")
                .unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"packed points").unwrap();
        }

        let artefacts = artefacts(&root).unwrap();

        assert_eq!(artefacts.len(), 1);
        assert_eq!(artefacts[0].artifact, "crossings");
        assert_eq!(artefacts[0].versions.len(), 2);
        assert!(
            artefacts[0].versions[0].version < artefacts[0].versions[1].version,
            "versions come back oldest first"
        );
        assert_eq!(
            artefacts[0].versions[0].contents,
            Contents {
                files: 1,
                rows: 0,
                bytes: 13,
            }
        );
    }

    #[test]
    fn a_store_with_no_gold_layer_holds_no_artefacts() {
        let tmp = tempfile::tempdir().unwrap();

        assert!(artefacts(&Root::new(tmp.path())).unwrap().is_empty());
    }
}
