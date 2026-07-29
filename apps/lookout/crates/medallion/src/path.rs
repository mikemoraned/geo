//! Building paths into the store: `<root>/<layer>/<dataset>/<key=value>…/<file>.parquet`.

use std::collections::HashSet;
use std::fmt::Display;
use std::path::{Path, PathBuf};

use arrow::array::RecordBatch;
use chrono::{DateTime, NaiveDate, Utc};
use datafusion::execution::SendableRecordBatchStream;

use crate::dataset::DatasetSpec;
use crate::geo::{write_geo_batches, write_geo_stream, GeoError};
use crate::layer::AppendOnly;
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
    AppendOnly(#[from] AppendOnly),
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("listing the partitions of {path}: {source}")]
    List {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("removing the partition {path}: {source}")]
    Remove {
        path: String,
        #[source]
        source: std::io::Error,
    },
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

    /// `~/Data/geo/lookout/medallion`, used when no root is given.
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join("Data/geo/lookout/medallion")
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Start building a path into `dataset`.
    pub fn dataset(&self, dataset: DatasetSpec) -> Dataset {
        Dataset {
            root: self.0.clone(),
            spec: dataset,
            partitions: Vec::new(),
        }
    }

    /// Start building a path into the dataset `T`'s rows make up, for a writer that names
    /// the rows it holds rather than the dataset they belong to.
    pub fn rows_of<T: Row>(&self) -> Dataset {
        self.dataset(T::DATASET)
    }
}

impl Default for Root {
    fn default() -> Self {
        Self::new(Self::default_path())
    }
}

/// A location within one dataset: which dataset, and the partitions chosen so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dataset {
    root: PathBuf,
    spec: DatasetSpec,
    partitions: Vec<Partition>,
}

impl Dataset {
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

    /// Whether this dataset's data may be replaced or deleted, as its layer decides.
    ///
    /// Checked on every path that replaces or deletes, so the append-only rule of the layers
    /// holding observations is enforced here rather than remembered by each caller. Appends
    /// need no such check: one refuses to land on a file that already exists.
    fn permit_replacement(&self) -> Result<(), AppendOnly> {
        if self.spec.layer.permits_replacement() {
            Ok(())
        } else {
            Err(AppendOnly {
                layer: self.layer(),
                dataset: self.name().to_string(),
            })
        }
    }

    /// The layer this dataset lives in.
    pub fn layer(&self) -> &'static str {
        self.spec.layer.as_str()
    }

    /// The dataset's name.
    pub fn name(&self) -> &'static str {
        self.spec.name
    }

    /// The directory the partitions resolve to.
    pub fn dir(&self) -> PathBuf {
        let mut dir = self
            .root
            .join(self.spec.layer.as_str())
            .join(self.spec.name);
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

    /// Replace this partition's contents with `batches`.
    pub async fn replace_with(&self, batches: &[RecordBatch]) -> Result<PathBuf, WriteError> {
        self.permit_replacement()?;
        let path = self.partition_file();
        write_batches(&path, batches).await?;
        Ok(path)
    }

    /// Replace this partition's contents with `batches`, as GeoParquet.
    pub async fn replace_with_geo(&self, batches: &[RecordBatch]) -> Result<PathBuf, GeoError> {
        self.permit_replacement().map_err(WriteError::from)?;
        let path = self.partition_file();
        write_geo_batches(&path, batches).await?;
        Ok(path)
    }

    /// Replace this partition's contents with a query's results, as GeoParquet, writing
    /// them as they arrive rather than holding them all first.
    pub async fn replace_with_geo_stream(
        &self,
        batches: SendableRecordBatchStream,
    ) -> Result<Written, GeoError> {
        self.permit_replacement().map_err(WriteError::from)?;
        let path = self.partition_file();
        let rows = write_geo_stream(&path, batches).await?;
        Ok(Written { path, rows })
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
        self.permit_replacement()?;
        let mut written = HashSet::new();
        for (date, batch) in days {
            let partition = self.clone().on_date(*date)?;
            partition
                .replace_with_geo(std::slice::from_ref(batch))
                .await?;
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
        self.permit_replacement()?;
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

    /// A named parquet file within [`Self::dir`].
    fn file(&self, stem: &str) -> PathBuf {
        self.dir().join(format!("{stem}.parquet"))
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    use crate::layer::Layer;

    const SENSOR_READING: DatasetSpec =
        DatasetSpec::partitioned(Layer::Bronze, "sensor_reading", "ingested_date");
    const SESSION: DatasetSpec = DatasetSpec::partitioned(Layer::Silver, "session", "start_date");
    const MOTIS_SEGMENT: DatasetSpec =
        DatasetSpec::partitioned(Layer::Bronze, "motis_segment", "polled_date");
    const OVERTURE_EXTRACT: DatasetSpec =
        DatasetSpec::partitioned(Layer::Bronze, "overture_extract", "extract_id");
    const EXTRACT_MANIFEST: DatasetSpec =
        DatasetSpec::unpartitioned(Layer::Bronze, "extract_manifest");

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

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, day).unwrap()
    }

    /// The partition directories of `dataset`, by name.
    fn partitions_of(dataset: &Dataset) -> Vec<String> {
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

    /// The layers holding what was observed cannot be re-derived, so replacing or sweeping
    /// one is refused here rather than left to every caller to avoid.
    #[tokio::test]
    async fn an_append_only_layer_refuses_to_be_replaced_or_swept() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let observed = Root::new(tmp.path()).dataset(SENSOR_READING);
        let at = Utc.with_ymd_and_hms(2026, 7, 26, 9, 0, 0).unwrap();
        observed
            .clone()
            .on_date(date(26))
            .expect("date")
            .append(at, &[geo_batch()])
            .await
            .expect("append");

        let replaced = observed.replace_dates_geo(&[(date(26), geo_batch())]).await;
        let swept = observed
            .retain_partitions("ingested_date", &["2026-07-26"])
            .await;
        let overwritten = observed
            .clone()
            .on_date(date(26))
            .expect("date")
            .replace_with(&[geo_batch()])
            .await;

        assert!(matches!(replaced, Err(ReplaceError::AppendOnly(_))));
        assert!(matches!(swept, Err(ReplaceError::AppendOnly(_))));
        assert!(matches!(overwritten, Err(WriteError::AppendOnly(_))));
        assert_eq!(
            partitions_of(&observed),
            ["ingested_date=2026-07-26"],
            "the appended partition is still there"
        );
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

    #[test]
    fn the_default_root_is_an_absolute_path_under_the_home_directory() {
        let default = Root::default_path();

        assert!(
            default.ends_with("Data/geo/lookout/medallion"),
            "unexpected default root: {}",
            default.display()
        );
        assert!(
            default.is_absolute(),
            "default root should be absolute: {}",
            default.display()
        );
    }
}
