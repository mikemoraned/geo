//! Building paths into the store: `<root>/<layer>/<dataset>/<key=value>…/<file>.parquet`.

use std::fmt::Display;
use std::path::{Path, PathBuf};

use arrow::array::RecordBatch;
use chrono::{DateTime, NaiveDate, Utc};

use crate::geo::{write_geo_batches, GeoError};
use crate::layer::Layer;
use crate::partition::{Partition, PathError, DATE_FORMAT};
use crate::write::{write_batches, WriteError};

/// Batch file names: compact UTC, so they sort chronologically and contain no `:`.
const BATCH_STEM_FORMAT: &str = "%Y%m%dT%H%M%SZ";

/// The file a rebuilt partition holds. A partition derived wholesale is one file, so its
/// name carries no information and never varies.
const PARTITION_STEM: &str = "part-0";

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

    /// Start building a path into the dataset `name` within `layer`.
    pub fn dataset(&self, layer: Layer, name: impl Into<String>) -> Dataset {
        Dataset {
            root: self.0.clone(),
            layer,
            name: name.into(),
            partitions: Vec::new(),
        }
    }
}

impl Default for Root {
    fn default() -> Self {
        Self::new(Self::default_path())
    }
}

/// A location within one dataset: its layer, its name, and the partitions chosen so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dataset {
    root: PathBuf,
    layer: Layer,
    name: String,
    partitions: Vec<Partition>,
}

impl Dataset {
    /// Append a Hive partition, validating the key and value against the store's rules.
    pub fn partition(mut self, key: &str, value: impl Display) -> Result<Self, PathError> {
        self.partitions.push(Partition::new(key, value)?);
        Ok(self)
    }

    /// Append a date-valued partition, formatted `YYYY-MM-DD`.
    pub fn date_partition(self, key: &str, date: NaiveDate) -> Result<Self, PathError> {
        self.partition(key, date.format(DATE_FORMAT))
    }

    /// The directory the partitions resolve to.
    pub fn dir(&self) -> PathBuf {
        let mut dir = self.root.join(self.layer.as_str()).join(&self.name);
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

    /// A named parquet file within [`Self::dir`].
    fn file(&self, stem: &str) -> PathBuf {
        self.dir().join(format!("{stem}.parquet"))
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn root() -> Root {
        Root::new("/store")
    }

    #[test]
    fn an_unpartitioned_dataset_is_root_layer_name() {
        let dir = root().dataset(Layer::Bronze, "extract_manifest").dir();

        assert_eq!(dir.to_str().unwrap(), "/store/bronze/extract_manifest");
    }

    #[test]
    fn partitions_are_directories_in_the_order_added() {
        let dir = root()
            .dataset(Layer::Bronze, "sensor_reading")
            .partition("sensor", "gps")
            .unwrap()
            .date_partition(
                "ingested_date",
                NaiveDate::from_ymd_opt(2026, 7, 26).unwrap(),
            )
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
            .dataset(Layer::Silver, "session")
            .date_partition("start_date", NaiveDate::from_ymd_opt(2026, 1, 2).unwrap())
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

        let path = root()
            .dataset(Layer::Bronze, "motis_segment")
            .batch_file(at);

        assert_eq!(
            path.to_str().unwrap(),
            "/store/bronze/motis_segment/20260726T140530Z.parquet"
        );
    }

    #[test]
    fn batch_files_from_different_instants_do_not_collide() {
        let dataset = root().dataset(Layer::Bronze, "motis_segment");

        let first = dataset
            .clone()
            .batch_file(Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 30).unwrap());
        let second = dataset.batch_file(Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 31).unwrap());

        assert_ne!(first, second);
    }

    #[test]
    fn an_invalid_partition_is_rejected_rather_than_encoded_into_the_path() {
        let dataset = root().dataset(Layer::Bronze, "sensor_reading");

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
