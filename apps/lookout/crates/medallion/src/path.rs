//! Building paths into the store: `<root>/<layer>/<dataset>/<key=value>…/<file>.parquet`.

use std::collections::HashSet;
use std::fmt::Display;
use std::path::{Path, PathBuf};

use arrow::array::RecordBatch;
use chrono::{DateTime, NaiveDate, Utc};
use datafusion::execution::SendableRecordBatchStream;

use crate::dataset::DatasetSpec;
use crate::geo::{write_geo_batches, write_geo_stream, GeoError};
use crate::layer::{LayerKind, Replaceable};
use crate::partition::{Partition, PathError, DATE_FORMAT};
use crate::rows::{batch, Row, RowError};
use crate::write::{write_batches, WriteError};

/// Failure appending rows to a dataset.
#[derive(Debug, thiserror::Error)]
pub enum AppendError {
    #[error(transparent)]
    Rows(#[from] RowError),
    #[error(transparent)]
    Write(#[from] WriteError),
}

/// Batch file names: compact UTC, so they sort chronologically and contain no `:`.
///
/// Millisecond precision, because the name is what keeps one write from landing on another:
/// a writer that batches — a drain, a backfill — issues several writes in quick succession,
/// and at second resolution they would collide.
const BATCH_STEM_FORMAT: &str = "%Y%m%dT%H%M%S%3fZ";

/// The file a wholly derived partition holds. Such a partition is one file, so its name
/// carries no information and never varies.
const PARTITION_STEM: &str = "part-0";

/// Failure replacing a dataset's partitions.
#[derive(Debug, thiserror::Error)]
pub enum ReplaceError {
    #[error(transparent)]
    Geo(#[from] GeoError),
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("listing the partitions of {path}: {source}")]
    List {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Write(#[from] WriteError),
    #[error("removing the partition {path}: {source}")]
    Remove {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Whether a partition's file holds geometry, and so which encoder writes it. A dataset
/// with no geometry column cannot be written as GeoParquet, which describes the geometry
/// columns of the file it is writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Geo,
    Plain,
}

/// What replacing a dataset's partitions did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Replaced {
    /// Partitions the run wrote.
    pub written: usize,
    /// Partitions the run no longer produces rows for, and so deleted.
    pub removed: usize,
}

impl std::ops::AddAssign for Replaced {
    /// Accumulates what several runs over one dataset did, for a caller replacing its
    /// partitions in more than one pass.
    fn add_assign(&mut self, other: Self) {
        self.written += other.written;
        self.removed += other.removed;
    }
}

/// What a write left in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub path: PathBuf,
    pub rows: usize,
}

/// The root of a medallion store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root(PathBuf);

impl Root {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// The store in the repo this was run from: `data/medallion` under the workspace root.
    ///
    /// Kept in the repo so the layers that cannot be re-derived are versioned alongside the
    /// code that wrote them. The workspace is found by walking up from the working directory
    /// for the manifest that declares it, as cargo does, rather than by resolving a relative
    /// path against wherever a binary happens to have been started — that would quietly make
    /// a second store instead of finding the one that exists.
    pub fn default_path() -> Result<PathBuf, StoreNotFound> {
        Ok(workspace_root()?.join(STORE_IN_REPO))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Start building a path into `dataset`.
    pub fn dataset<L: LayerKind>(&self, dataset: DatasetSpec<L>) -> Dataset<L> {
        Dataset {
            root: self.0.clone(),
            spec: dataset,
            partitions: Vec::new(),
        }
    }

    /// Start building a path into the dataset `T`'s rows make up, for a writer that names
    /// the rows it holds rather than the dataset they belong to.
    pub fn rows_of<T: Row>(&self) -> Dataset<T::Layer> {
        self.dataset(T::DATASET)
    }
}

/// Where the store sits within the repo.
const STORE_IN_REPO: &str = "data/medallion";

/// The manifest naming the workspace, and the section that makes it one.
const WORKSPACE_MANIFEST: &str = "Cargo.toml";
const WORKSPACE_SECTION: &str = "[workspace]";

/// No workspace above the working directory, and so nowhere the store could be.
#[derive(Debug, thiserror::Error)]
#[error(
    "no {WORKSPACE_MANIFEST} declaring {WORKSPACE_SECTION} at or above {from}, so the store's \
     location cannot be worked out; pass --medallion-root to say where it is"
)]
pub struct StoreNotFound {
    pub from: String,
}

/// The workspace the working directory sits in.
fn workspace_root() -> Result<PathBuf, StoreNotFound> {
    let from = std::env::current_dir().unwrap_or_default();
    let declares_workspace = |dir: &Path| {
        std::fs::read_to_string(dir.join(WORKSPACE_MANIFEST))
            .is_ok_and(|manifest| manifest.contains(WORKSPACE_SECTION))
    };

    from.ancestors()
        .find(|dir| declares_workspace(dir))
        .map(Path::to_path_buf)
        .ok_or_else(|| StoreNotFound {
            from: from.display().to_string(),
        })
}

/// A location within one dataset: which dataset, and the partitions chosen so far.
///
/// `L` is the dataset's layer, so what can be done to it follows from where it lives: the
/// operations that rewrite or delete are implemented for [`Replaceable`] layers only, and a
/// bronze dataset simply does not have them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dataset<L> {
    root: PathBuf,
    spec: DatasetSpec<L>,
    partitions: Vec<Partition>,
}

impl<L: LayerKind> Dataset<L> {
    /// Append a Hive partition, validating the key and value against the store's rules.
    pub fn partition(mut self, key: &str, value: impl Display) -> Result<Self, PathError> {
        self.partitions.push(Partition::new(key, value)?);
        Ok(self)
    }

    /// Append a date-valued partition under the dataset's own key, formatted `YYYY-MM-DD`.
    pub fn on_date(self, date: NaiveDate) -> Result<Self, PathError> {
        let key = self.own_key()?;
        self.partition(key, date.format(DATE_FORMAT))
    }

    /// Append an id-valued partition under the dataset's own key, for a dataset whose
    /// unit of writing is identified rather than dated.
    pub fn for_id(self, id: impl Display) -> Result<Self, PathError> {
        let key = self.own_key()?;
        self.partition(key, id)
    }

    /// The dataset's declared partition key, or a failure naming the dataset that has
    /// none — partitioning an unpartitioned dataset is a definition mismatch, not a path
    /// the caller can fix by escaping something.
    fn own_key(&self) -> Result<&'static str, PathError> {
        self.spec
            .partition_key
            .ok_or_else(|| PathError::Unpartitioned(self.spec.name.to_string()))
    }

    /// The layer this dataset lives in.
    pub fn layer(&self) -> &'static str {
        L::LAYER.as_str()
    }

    /// The dataset's name.
    pub fn name(&self) -> &'static str {
        self.spec.name
    }

    /// The directory the partitions resolve to.
    pub fn dir(&self) -> PathBuf {
        let mut dir = self.root.join(L::LAYER.as_str()).join(self.spec.name);
        dir.extend(self.partitions.iter().map(Partition::to_string));
        dir
    }

    /// The file one batch captured at `at` lands in: a new file per write, named for the
    /// instant of the write, so an earlier capture is never rewritten.
    pub fn batch_file(&self, at: DateTime<Utc>) -> PathBuf {
        self.file(&at.format(BATCH_STEM_FORMAT).to_string())
    }

    /// The file this partition's contents live in, replaced whenever they are derived again.
    pub fn partition_file(&self) -> PathBuf {
        self.file(PARTITION_STEM)
    }

    /// Append `batches` as the capture made at `at`, leaving earlier captures untouched.
    ///
    /// A capture already written at `at` is not replaced: an append that would land on an
    /// existing file fails instead, since these layers are immutable and the rows already
    /// there are not this caller's to discard.
    pub async fn append(
        &self,
        at: DateTime<Utc>,
        batches: &[RecordBatch],
    ) -> Result<PathBuf, WriteError> {
        let path = self.batch_file(at);
        if path.exists() {
            return Err(WriteError::Exists {
                path: path.display().to_string(),
            });
        }
        write_batches(&path, batches).await?;
        Ok(path)
    }

    /// Append `rows` as the capture made at `at`.
    ///
    /// Having nothing to append is not a failure: it writes no file and reports `0`, so a
    /// capture that saw no rows of this kind leaves no empty file behind.
    pub async fn append_rows<T: Row>(
        &self,
        at: DateTime<Utc>,
        rows: &[T],
    ) -> Result<usize, AppendError> {
        if rows.is_empty() {
            return Ok(0);
        }
        self.append(at, &[batch(rows)?]).await?;
        Ok(rows.len())
    }

    /// Append a query's results as the capture made at `at`, as GeoParquet, writing them as
    /// they arrive rather than holding them all first.
    ///
    /// A capture already written at `at` is not replaced, and a query matching nothing still
    /// writes a readable, correctly typed file rather than failing — a partition that
    /// legitimately holds no rows is a result.
    pub async fn append_geo_stream(
        &self,
        at: DateTime<Utc>,
        batches: SendableRecordBatchStream,
    ) -> Result<Written, GeoError> {
        let path = self.batch_file(at);
        if path.exists() {
            return Err(WriteError::Exists {
                path: path.display().to_string(),
            }
            .into());
        }
        let rows = write_geo_stream(&path, batches).await?;
        Ok(Written { path, rows })
    }

    /// A named parquet file within [`Self::dir`].
    fn file(&self, stem: &str) -> PathBuf {
        self.dir().join(format!("{stem}.parquet"))
    }
}

/// What may be done to a dataset only where its layer permits data to be replaced.
///
/// These are the operations a rebuild needs: rewriting a partition, and deleting the
/// partitions a run no longer produces. They exist for silver and gold, so
/// `root.rows_of::<RawSampleRow>().replace_dates_geo(…)` is not a call that can be written —
/// bronze and landing hold what was observed, and nothing can derive that back.
impl<L: Replaceable> Dataset<L> {
    /// Replace this partition's contents with `batches`.
    pub async fn replace_with(&self, batches: &[RecordBatch]) -> Result<PathBuf, WriteError> {
        let path = self.partition_file();
        write_batches(&path, batches).await?;
        Ok(path)
    }

    /// Replace this partition's contents with `batches`, as GeoParquet.
    pub async fn replace_with_geo(&self, batches: &[RecordBatch]) -> Result<PathBuf, GeoError> {
        let path = self.partition_file();
        write_geo_batches(&path, batches).await?;
        Ok(path)
    }

    /// Replace this dataset's partitions with one file per dated batch, as GeoParquet.
    ///
    /// A partition the run produces no rows for is **deleted**: a silver dataset is derived
    /// wholesale from its source, so a partition left standing is a claim the derivation no
    /// longer makes, and a reader has no way to tell it apart from a current one. Only
    /// directories under this dataset's own partition key are considered, so nothing
    /// outside what this dataset writes is ever removed.
    pub async fn replace_dates_geo(
        &self,
        days: &[(NaiveDate, RecordBatch)],
    ) -> Result<Replaced, ReplaceError> {
        self.replace_dates_as(days, Encoding::Geo).await
    }

    /// Replace this dataset's partitions with one file per dated batch, as plain parquet,
    /// for a dataset holding no geometry. Sweeps as [`Self::replace_dates_geo`] does.
    pub async fn replace_dates(
        &self,
        days: &[(NaiveDate, RecordBatch)],
    ) -> Result<Replaced, ReplaceError> {
        self.replace_dates_as(days, Encoding::Plain).await
    }

    async fn replace_dates_as(
        &self,
        days: &[(NaiveDate, RecordBatch)],
        encoding: Encoding,
    ) -> Result<Replaced, ReplaceError> {
        let mut written = HashSet::new();
        for (date, batch) in days {
            let partition = self.clone().on_date(*date)?;
            let batch = std::slice::from_ref(batch);
            match encoding {
                Encoding::Geo => partition.replace_with_geo(batch).await.map(|_| ())?,
                Encoding::Plain => partition.replace_with(batch).await.map(|_| ())?,
            }
            written.insert(partition.dir());
        }

        let removed = self.sweep(self.own_key()?, &written).await?;
        Ok(Replaced {
            written: written.len(),
            removed,
        })
    }

    /// Delete every partition under `key` whose value is not among `values`, reporting how
    /// many went.
    ///
    /// This is the level above [`Self::replace_dates_geo`], which sweeps within one value of
    /// `key` because that is the level it is given. A caller that knows every value the run
    /// produced sweeps the level itself, so a value the run no longer produces anything for
    /// leaves nothing behind — and a reader listing what values a dataset holds sees the
    /// answer the last run gave rather than the union of every run so far.
    ///
    /// It follows that the caller has to have derived the whole dataset: a run over some of
    /// the values would read as a run that produced nothing for the rest.
    pub async fn retain_partitions<V: Display>(
        &self,
        key: &str,
        values: &[V],
    ) -> Result<usize, ReplaceError> {
        let keep = values
            .iter()
            .map(|value| Ok(self.dir().join(Partition::new(key, value)?.to_string())))
            .collect::<Result<HashSet<PathBuf>, PathError>>()?;

        self.sweep(key, &keep).await
    }

    /// Delete every directory of this dataset named for `key` except `keep`, reporting how
    /// many went.
    async fn sweep(&self, key: &str, keep: &HashSet<PathBuf>) -> Result<usize, ReplaceError> {
        let dir = self.dir();
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            // A dataset nothing has been written to yet has no partitions to sweep.
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(source) => {
                return Err(ReplaceError::List {
                    path: dir.display().to_string(),
                    source,
                })
            }
        };

        let mut removed = 0;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|source| ReplaceError::List {
                path: dir.display().to_string(),
                source,
            })?
        {
            let path = entry.path();
            let is_stale = path.is_dir()
                && !keep.contains(&path)
                && entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{key}="));
            if is_stale {
                tokio::fs::remove_dir_all(&path)
                    .await
                    .map_err(|source| ReplaceError::Remove {
                        path: path.display().to_string(),
                        source,
                    })?;
                removed += 1;
            }
        }

        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    use crate::layer::layers;

    const SENSOR_READING: DatasetSpec<layers::Bronze> =
        DatasetSpec::partitioned("sensor_reading", "ingested_date");
    const SESSION: DatasetSpec<layers::Silver> = DatasetSpec::partitioned("session", "start_date");
    const MOTIS_SEGMENT: DatasetSpec<layers::Bronze> =
        DatasetSpec::partitioned("motis_segment", "polled_date");
    const OVERTURE_EXTRACT: DatasetSpec<layers::Bronze> =
        DatasetSpec::partitioned("overture_extract", "extract_id");
    const EXTRACT_MANIFEST: DatasetSpec<layers::Bronze> =
        DatasetSpec::unpartitioned("extract_manifest");

    fn root() -> Root {
        Root::new("/store")
    }

    #[test]
    fn an_unpartitioned_dataset_is_root_layer_name() {
        let dir = root().dataset(EXTRACT_MANIFEST).dir();

        assert_eq!(dir.to_str().unwrap(), "/store/bronze/extract_manifest");
    }

    /// An extract keeps the upstream's own layout below its id, so the id partition is
    /// added under the dataset's key and the upstream's keys follow it.
    #[test]
    fn an_id_partition_uses_the_datasets_own_key() {
        let dir = root()
            .dataset(OVERTURE_EXTRACT)
            .for_id("20260727T101500Z")
            .unwrap()
            .partition("theme", "transportation")
            .unwrap()
            .dir();

        assert_eq!(
            dir.to_str().unwrap(),
            "/store/bronze/overture_extract/extract_id=20260727T101500Z/theme=transportation"
        );
    }

    #[test]
    fn an_unpartitioned_dataset_cannot_be_partitioned_under_a_key_it_does_not_have() {
        let dataset = root().dataset(EXTRACT_MANIFEST);

        assert_eq!(
            dataset.for_id("20260727T101500Z").unwrap_err(),
            PathError::Unpartitioned("extract_manifest".to_string())
        );
    }

    #[test]
    fn partitions_are_directories_in_the_order_added() {
        let dir = root()
            .dataset(SENSOR_READING)
            .partition("sensor", "gps")
            .unwrap()
            .on_date(NaiveDate::from_ymd_opt(2026, 7, 26).unwrap())
            .unwrap()
            .dir();

        assert_eq!(
            dir.to_str().unwrap(),
            "/store/bronze/sensor_reading/sensor=gps/ingested_date=2026-07-26"
        );
    }

    #[test]
    fn a_partition_file_sits_under_the_partition_directory_with_a_parquet_extension() {
        let path = root()
            .dataset(SESSION)
            .on_date(NaiveDate::from_ymd_opt(2026, 1, 2).unwrap())
            .unwrap()
            .partition_file();

        assert_eq!(
            path.to_str().unwrap(),
            "/store/silver/session/start_date=2026-01-02/part-0.parquet"
        );
    }

    #[test]
    fn a_batch_file_is_named_for_the_instant_of_the_write() {
        let at = Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 30).unwrap();

        let path = root().dataset(MOTIS_SEGMENT).batch_file(at);

        assert_eq!(
            path.to_str().unwrap(),
            "/store/bronze/motis_segment/20260726T140530000Z.parquet"
        );
    }

    #[test]
    fn batch_files_from_different_instants_do_not_collide() {
        let dataset = root().dataset(MOTIS_SEGMENT);

        let first = dataset
            .clone()
            .batch_file(Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 30).unwrap());
        let second = dataset.batch_file(Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 31).unwrap());

        assert_ne!(first, second);
    }

    /// Writes within one second are the normal case for a writer that batches, so the name
    /// has to separate them: at second resolution they would name one file and the second
    /// write would land on the first.
    #[test]
    fn batch_files_from_instants_in_the_same_second_do_not_collide() {
        let dataset = root().dataset(MOTIS_SEGMENT);
        let at = Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 30).unwrap();

        let first = dataset.clone().batch_file(at);
        let second = dataset.batch_file(at + chrono::Duration::milliseconds(1));

        assert_ne!(first, second);
    }

    #[test]
    fn an_invalid_partition_is_rejected_rather_than_encoded_into_the_path() {
        let dataset = root().dataset(SENSOR_READING);

        assert!(dataset.clone().partition("ingestedDate", "gps").is_err());
        assert!(dataset.partition("sensor", "gps/accel").is_err());
    }

    /// One row of a dataset whose only column is a point, so the batch is writable as
    /// GeoParquet the way a real silver batch is.
    fn geo_batch() -> RecordBatch {
        let field = crate::geo::wkb_field("geometry").expect("field");
        let (field, array) =
            crate::geo::wkb_column(field, &[geo_types::Point::new(13.4, 52.5)]).expect("column");
        RecordBatch::try_new(
            std::sync::Arc::new(arrow::datatypes::Schema::new(vec![field])),
            vec![array],
        )
        .expect("batch")
    }

    /// `batch` as the stream a query would have produced it as.
    async fn stream_of(batch: RecordBatch) -> SendableRecordBatchStream {
        let ctx = datafusion::prelude::SessionContext::new();
        ctx.read_batch(batch)
            .expect("read batch")
            .execute_stream()
            .await
            .expect("stream")
    }

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, day).unwrap()
    }

    /// The partition directories of `dataset`, by name.
    fn partitions_of<L: LayerKind>(dataset: &Dataset<L>) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dataset.dir())
            .expect("dataset dir")
            .map(|entry| entry.expect("entry").file_name().to_string_lossy().into())
            .collect();
        names.sort();
        names
    }

    #[tokio::test]
    async fn replacing_writes_one_partition_per_date() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dataset = Root::new(tmp.path()).dataset(SESSION);

        let replaced = dataset
            .replace_dates_geo(&[(date(26), geo_batch()), (date(27), geo_batch())])
            .await
            .expect("replace");

        assert_eq!(
            replaced,
            Replaced {
                written: 2,
                removed: 0
            }
        );
        assert_eq!(
            partitions_of(&dataset),
            ["start_date=2026-07-26", "start_date=2026-07-27"]
        );
    }

    /// A silver dataset is derived wholesale, so a partition the run no longer produces
    /// rows for is a claim the derivation has withdrawn — it goes rather than lingering as
    /// something a reader cannot tell from current.
    #[tokio::test]
    async fn a_partition_the_run_no_longer_produces_rows_for_is_deleted() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dataset = Root::new(tmp.path()).dataset(SESSION);
        dataset
            .replace_dates_geo(&[(date(26), geo_batch()), (date(27), geo_batch())])
            .await
            .expect("first run");

        let replaced = dataset
            .replace_dates_geo(&[(date(26), geo_batch())])
            .await
            .expect("second run");

        assert_eq!(
            replaced,
            Replaced {
                written: 1,
                removed: 1
            }
        );
        assert_eq!(partitions_of(&dataset), ["start_date=2026-07-26"]);
    }

    #[tokio::test]
    async fn replacing_a_dataset_with_nothing_leaves_no_partitions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dataset = Root::new(tmp.path()).dataset(SESSION);
        dataset
            .replace_dates_geo(&[(date(26), geo_batch())])
            .await
            .expect("first run");

        let replaced = dataset.replace_dates_geo(&[]).await.expect("second run");

        assert_eq!(
            replaced,
            Replaced {
                written: 0,
                removed: 1
            }
        );
        assert!(partitions_of(&dataset).is_empty());
    }

    /// The sweep is bounded by the dataset's own partition key, so anything else sharing
    /// the directory — another key, a file — is not this dataset's to delete.
    #[tokio::test]
    async fn only_this_datasets_own_partitions_are_swept() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dataset = Root::new(tmp.path()).dataset(SESSION);
        dataset
            .replace_dates_geo(&[(date(26), geo_batch())])
            .await
            .expect("first run");
        std::fs::create_dir(dataset.dir().join("region=de")).expect("other partition");
        std::fs::write(dataset.dir().join("NOTES.md"), "kept").expect("stray file");

        let replaced = dataset
            .replace_dates_geo(&[(date(27), geo_batch())])
            .await
            .expect("second run");

        assert_eq!(replaced.removed, 1, "only the dated partition goes");
        assert_eq!(
            partitions_of(&dataset),
            ["NOTES.md", "region=de", "start_date=2026-07-27"]
        );
    }

    /// A value the run no longer produces rows for leaves nothing behind at the level above
    /// the dates, which is the level its caller names rather than this one.
    #[tokio::test]
    async fn a_value_the_run_no_longer_names_is_swept_from_the_level_above() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dataset = Root::new(tmp.path()).dataset(SESSION);
        for country in ["DE", "FR"] {
            dataset
                .clone()
                .partition("country", country)
                .expect("country")
                .replace_dates_geo(&[(date(26), geo_batch())])
                .await
                .expect("write");
        }

        let removed = dataset
            .retain_partitions("country", &["DE"])
            .await
            .expect("retain");

        assert_eq!(removed, 1);
        assert_eq!(partitions_of(&dataset), ["country=DE"]);
    }

    /// Retaining nothing empties the dataset: a run that derived no values claims none.
    #[tokio::test]
    async fn retaining_no_values_sweeps_every_partition_of_the_level() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dataset = Root::new(tmp.path()).dataset(SESSION);
        dataset
            .clone()
            .partition("country", "DE")
            .expect("country")
            .replace_dates_geo(&[(date(26), geo_batch())])
            .await
            .expect("write");

        let removed = dataset
            .retain_partitions::<&str>("country", &[])
            .await
            .expect("retain");

        assert_eq!(removed, 1);
        assert!(partitions_of(&dataset).is_empty());
    }

    /// The sweep is bounded by the key it is given, so a partition of the dataset's own
    /// dated key is not something a sweep of another level removes.
    #[tokio::test]
    async fn retaining_one_key_leaves_partitions_of_another_alone() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dataset = Root::new(tmp.path()).dataset(SESSION);
        dataset
            .replace_dates_geo(&[(date(26), geo_batch())])
            .await
            .expect("write");

        let removed = dataset
            .retain_partitions("country", &["DE"])
            .await
            .expect("retain");

        assert_eq!(removed, 0);
        assert_eq!(partitions_of(&dataset), ["start_date=2026-07-26"]);
    }

    /// An append-only layer still takes a streamed write, which is how a query's results
    /// reach bronze: what it may not do is land on a capture already written.
    #[tokio::test]
    async fn an_append_only_layer_takes_a_stream_but_not_twice() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let partition = Root::new(tmp.path())
            .dataset(SENSOR_READING)
            .on_date(date(26))
            .expect("date");
        let at = Utc.with_ymd_and_hms(2026, 7, 26, 9, 0, 0).unwrap();

        let written = partition
            .append_geo_stream(at, stream_of(geo_batch()).await)
            .await
            .expect("append a stream");
        let again = partition
            .append_geo_stream(at, stream_of(geo_batch()).await)
            .await;

        assert_eq!(written.rows, 1);
        assert_eq!(written.path, partition.batch_file(at));
        assert!(matches!(
            again,
            Err(GeoError::Write(WriteError::Exists { .. }))
        ));
    }

    /// A dataset nothing has been written to yet has no partitions to sweep, rather than a
    /// missing directory to fail on.
    #[tokio::test]
    async fn replacing_a_dataset_that_does_not_exist_yet_writes_it() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let replaced = Root::new(tmp.path())
            .dataset(SESSION)
            .replace_dates_geo(&[(date(26), geo_batch())])
            .await
            .expect("replace");

        assert_eq!(
            replaced,
            Replaced {
                written: 1,
                removed: 0
            }
        );
    }

    /// The default store is the repo's own, found by walking up rather than by guessing a
    /// path relative to wherever this was run.
    #[test]
    fn the_default_root_is_the_store_in_the_repo() {
        let default = Root::default_path().expect("locate the store");

        assert!(
            default.ends_with("data/medallion"),
            "unexpected default root: {}",
            default.display()
        );
        assert!(
            default.is_absolute(),
            "default root should be absolute: {}",
            default.display()
        );
        assert!(
            default.starts_with(workspace_root().expect("locate the workspace")),
            "the store should sit in the workspace: {}",
            default.display()
        );
    }

    /// The workspace is the one this crate belongs to, whichever of its directories a test
    /// happens to run in.
    #[test]
    fn the_workspace_is_found_by_walking_up_from_the_working_directory() {
        let workspace = workspace_root().expect("locate the workspace");

        assert!(workspace.join("Cargo.toml").exists());
        assert!(std::fs::read_to_string(workspace.join("Cargo.toml"))
            .expect("read the manifest")
            .contains(WORKSPACE_SECTION));
    }
}
