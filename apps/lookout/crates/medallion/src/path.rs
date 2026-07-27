//! Building paths into the store: `<root>/<layer>/<dataset>/<key=value>…/<file>.parquet`.

use std::fmt::Display;
use std::path::{Path, PathBuf};

use arrow::array::RecordBatch;
use chrono::{DateTime, NaiveDate, Utc};
use datafusion::execution::SendableRecordBatchStream;

use crate::dataset::DatasetSpec;
use crate::geo::{write_geo_batches, write_geo_stream, GeoError};
use crate::partition::{Partition, PathError, DATE_FORMAT};
use crate::rows::{batch, RowError};
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
const BATCH_STEM_FORMAT: &str = "%Y%m%dT%H%M%SZ";

/// The file a rebuilt partition holds. A partition derived wholesale is one file, so its
/// name carries no information and never varies.
const PARTITION_STEM: &str = "part-0";

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

    /// The file this partition's contents live in, replaced whenever it is rebuilt.
    pub fn partition_file(&self) -> PathBuf {
        self.file(PARTITION_STEM)
    }

    /// Append `batches` as the capture made at `at`, leaving earlier captures untouched.
    pub async fn append(
        &self,
        at: DateTime<Utc>,
        batches: &[RecordBatch],
    ) -> Result<PathBuf, WriteError> {
        let path = self.batch_file(at);
        write_batches(&path, batches).await?;
        Ok(path)
    }

    /// Append `rows` as the capture made at `at`, naming the columns holding an instant
    /// (see [`crate::rows`]).
    ///
    /// Having nothing to append is not a failure: it writes no file and reports `0`, so a
    /// capture that saw no rows of this kind leaves no empty file behind.
    pub async fn append_rows<T>(
        &self,
        at: DateTime<Utc>,
        rows: &[T],
        instants: &[&str],
    ) -> Result<usize, AppendError>
    where
        T: serde::Serialize + for<'de> serde::Deserialize<'de>,
    {
        if rows.is_empty() {
            return Ok(0);
        }
        self.append(at, &[batch(rows, instants)?]).await?;
        Ok(rows.len())
    }

    /// Replace this partition's contents with `batches`.
    pub async fn rebuild(&self, batches: &[RecordBatch]) -> Result<PathBuf, WriteError> {
        let path = self.partition_file();
        write_batches(&path, batches).await?;
        Ok(path)
    }

    /// Replace this partition's contents with `batches`, as GeoParquet.
    pub async fn rebuild_geo(&self, batches: &[RecordBatch]) -> Result<PathBuf, GeoError> {
        let path = self.partition_file();
        write_geo_batches(&path, batches).await?;
        Ok(path)
    }

    /// Replace this partition's contents with a query's results, as GeoParquet, writing
    /// them as they arrive rather than holding them all first.
    pub async fn rebuild_geo_from(
        &self,
        batches: SendableRecordBatchStream,
    ) -> Result<Written, GeoError> {
        let path = self.partition_file();
        let rows = write_geo_stream(&path, batches).await?;
        Ok(Written { path, rows })
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
            "/store/bronze/motis_segment/20260726T140530Z.parquet"
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

    #[test]
    fn an_invalid_partition_is_rejected_rather_than_encoded_into_the_path() {
        let dataset = root().dataset(SENSOR_READING);

        assert!(dataset.clone().partition("ingestedDate", "gps").is_err());
        assert!(dataset.partition("sensor", "gps/accel").is_err());
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
