use std::collections::HashSet;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::RecordBatch;
use arrow::datatypes::Schema;
use chrono::{DateTime, NaiveDate, Utc};
use datafusion::execution::SendableRecordBatchStream;
use futures::TryStreamExt;
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectPath;
use object_store::{ObjectMeta, ObjectStore};

use crate::dataset::DatasetSpec;
use crate::geo::{GeoError, GeoParquetFile, write_geo_batches, write_geo_stream};
use crate::layer::{Layer, LayerKind, Replaceable, layers};
use crate::partition::{DATE_FORMAT, Partition, PathError};
use crate::rows::{Row, RowError, batch};
use crate::write::{WriteError, refuse_existing, write_batches};

#[derive(Debug, thiserror::Error)]
pub enum AppendError {
    #[error(transparent)]
    Rows(#[from] RowError),
    #[error(transparent)]
    Write(#[from] WriteError),
}

const BATCH_STEM_FORMAT: &str = "%Y%m%dT%H%M%S%3fZ";

const PARTITION_STEM: &str = "part-0";

fn batch_stem(at: DateTime<Utc>) -> String {
    at.format(BATCH_STEM_FORMAT).to_string()
}

fn file_name(stem: &str) -> String {
    format!("{stem}.parquet")
}

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

#[derive(Debug, thiserror::Error)]
pub enum LandingError {
    #[error("listing the files of {location}: {source}")]
    List {
        location: String,
        #[source]
        source: object_store::Error,
    },
    #[error("deleting {location}: {source}")]
    Delete {
        location: String,
        #[source]
        source: object_store::Error,
    },
}

#[derive(Debug, Clone)]
pub struct LandedFile {
    store: Arc<dyn ObjectStore>,
    location: ObjectPath,
}

impl LandedFile {
    pub fn location(&self) -> &ObjectPath {
        &self.location
    }

    pub async fn delete(self) -> Result<(), LandingError> {
        self.store
            .delete(&self.location)
            .await
            .map_err(|source| LandingError::Delete {
                location: self.location.to_string(),
                source,
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Geo,
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Replaced {
    pub written: usize,
    pub removed: usize,
}

impl std::ops::AddAssign for Replaced {
    fn add_assign(&mut self, other: Self) {
        self.written += other.written;
        self.removed += other.removed;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub path: PathBuf,
    pub rows: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root(PathBuf);

impl Root {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    pub fn default_path() -> Result<PathBuf, StoreNotFound> {
        Ok(workspace_root()?.join(STORE_IN_REPO))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn dataset<L: LayerKind>(&self, dataset: DatasetSpec<L>) -> Dataset<L> {
        Dataset {
            root: self.0.clone(),
            spec: dataset,
            partitions: Vec::new(),
        }
    }

    pub fn rows_of<T: Row>(&self) -> Dataset<T::Layer> {
        self.dataset(T::DATASET)
    }

    pub fn object_store(&self) -> Result<Arc<dyn ObjectStore>, object_store::Error> {
        Ok(Arc::new(LocalFileSystem::new_with_prefix(&self.0)?))
    }

    fn writable_store(&self) -> Result<Arc<dyn ObjectStore>, WriteError> {
        std::fs::create_dir_all(&self.0).map_err(|source| WriteError::Root {
            path: self.0.display().to_string(),
            source,
        })?;
        Ok(self.object_store()?)
    }

    pub fn gold_artefact(
        &self,
        artifact: &str,
        run: DateTime<Utc>,
        name: &str,
    ) -> Result<PathBuf, PathError> {
        let version = gold_version(run);
        Ok(self
            .0
            .join(Layer::Gold.as_str())
            .join(Partition::new(ARTIFACT, artifact)?.to_string())
            .join(Partition::new(VERSION, version)?.to_string())
            .join(name))
    }
}

#[must_use]
pub fn gold_version(run: DateTime<Utc>) -> String {
    batch_stem(run)
}

const ARTIFACT: &str = "artifact";
const VERSION: &str = "version";

const STORE_IN_REPO: &str = "data/medallion";

const WORKSPACE_MANIFEST: &str = "Cargo.toml";
const WORKSPACE_SECTION: &str = "[workspace]";

#[derive(Debug, thiserror::Error)]
#[error(
    "no {WORKSPACE_MANIFEST} declaring {WORKSPACE_SECTION} at or above {from}, so the store's \
     location cannot be worked out; pass --medallion-root to say where it is"
)]
pub struct StoreNotFound {
    pub from: String,
}

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

pub(crate) fn any_file_under(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry.path().is_dir() && any_file_under(&entry.path()) || entry.path().is_file()
        })
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dataset<L> {
    root: PathBuf,
    spec: DatasetSpec<L>,
    partitions: Vec<Partition>,
}

impl<L: LayerKind> Dataset<L> {
    pub fn partition(mut self, key: &str, value: impl Display) -> Result<Self, PathError> {
        self.partitions.push(Partition::new(key, value)?);
        Ok(self)
    }

    pub fn on_date(self, date: NaiveDate) -> Result<Self, PathError> {
        let key = self.own_key()?;
        self.partition(key, date.format(DATE_FORMAT))
    }

    pub fn for_id(self, id: impl Display) -> Result<Self, PathError> {
        let key = self.own_key()?;
        self.partition(key, id)
    }

    fn own_key(&self) -> Result<&'static str, PathError> {
        self.spec
            .partition_key
            .ok_or_else(|| PathError::Unpartitioned(self.spec.name.to_string()))
    }

    pub fn layer(&self) -> &'static str {
        L::LAYER.as_str()
    }

    pub fn name(&self) -> &'static str {
        self.spec.name
    }

    pub fn root(&self) -> Root {
        Root::new(self.root.clone())
    }

    pub fn location(&self) -> ObjectPath {
        [L::LAYER.as_str().to_string(), self.spec.name.to_string()]
            .into_iter()
            .chain(self.partitions.iter().map(Partition::to_string))
            .collect()
    }

    pub fn dir(&self) -> PathBuf {
        let mut dir = self.root.join(L::LAYER.as_str()).join(self.spec.name);
        dir.extend(self.partitions.iter().map(Partition::to_string));
        dir
    }

    pub fn is_filled(&self) -> bool {
        any_file_under(&self.dir())
    }

    pub fn batch_file(&self, at: DateTime<Utc>) -> PathBuf {
        self.file(&batch_stem(at))
    }

    pub fn partition_file(&self) -> PathBuf {
        self.file(PARTITION_STEM)
    }

    pub async fn append(
        &self,
        at: DateTime<Utc>,
        batches: &[RecordBatch],
    ) -> Result<PathBuf, WriteError> {
        let (store, location) = self.destination(&batch_stem(at))?;
        refuse_existing(store.as_ref(), &location).await?;
        write_batches(store, location, batches).await?;
        Ok(self.batch_file(at))
    }

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

    pub async fn append_geo_stream(
        &self,
        at: DateTime<Utc>,
        batches: SendableRecordBatchStream,
    ) -> Result<Written, GeoError> {
        let (store, location) = self.destination(&batch_stem(at))?;
        refuse_existing(store.as_ref(), &location).await?;
        let rows = write_geo_stream(store, location, batches).await?;
        Ok(Written {
            path: self.batch_file(at),
            rows,
        })
    }

    fn file(&self, stem: &str) -> PathBuf {
        self.dir().join(file_name(stem))
    }

    fn destination(&self, stem: &str) -> Result<(Arc<dyn ObjectStore>, ObjectPath), WriteError> {
        Ok((
            self.root().writable_store()?,
            self.location().child(file_name(stem)),
        ))
    }
}

impl Dataset<layers::Landing> {
    pub async fn landed_files(&self) -> Result<Vec<LandedFile>, LandingError> {
        let location = self.location();
        let list_error = |source| LandingError::List {
            location: location.to_string(),
            source,
        };
        let store = self.root().object_store().map_err(list_error)?;
        let listed: Vec<ObjectMeta> = match store.list(Some(&location)).try_collect().await {
            Err(object_store::Error::NotFound { .. }) => Vec::new(),
            listed => listed.map_err(list_error)?,
        };
        let mut locations: Vec<ObjectPath> = listed
            .into_iter()
            .map(|object| object.location)
            .filter(|location| location.extension() == Some("parquet"))
            .collect();
        locations.sort_unstable();
        Ok(locations
            .into_iter()
            .map(|location| LandedFile {
                store: Arc::clone(&store),
                location,
            })
            .collect())
    }
}

impl<L: Replaceable> Dataset<L> {
    pub async fn replace_with(&self, batches: &[RecordBatch]) -> Result<PathBuf, WriteError> {
        let (store, location) = self.destination(PARTITION_STEM)?;
        write_batches(store, location, batches).await?;
        Ok(self.partition_file())
    }

    pub(crate) fn create_geo_partition(&self, schema: &Schema) -> Result<GeoParquetFile, GeoError> {
        let (store, location) = self.destination(PARTITION_STEM)?;
        GeoParquetFile::create(store, location, schema)
    }

    pub async fn replace_with_geo(&self, batches: &[RecordBatch]) -> Result<PathBuf, GeoError> {
        let (store, location) = self.destination(PARTITION_STEM)?;
        write_geo_batches(store, location, batches).await?;
        Ok(self.partition_file())
    }

    pub async fn replace_dates_geo(
        &self,
        days: &[(NaiveDate, RecordBatch)],
    ) -> Result<Replaced, ReplaceError> {
        self.replace_dates_as(days, Encoding::Geo).await
    }

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

    async fn sweep(&self, key: &str, keep: &HashSet<PathBuf>) -> Result<usize, ReplaceError> {
        let dir = self.dir();
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(source) => {
                return Err(ReplaceError::List {
                    path: dir.display().to_string(),
                    source,
                });
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
    const POLL: DatasetSpec<layers::Landing> = DatasetSpec::partitioned("poll", "polled_date");

    fn root() -> Root {
        Root::new("/store")
    }

    fn land(dataset: &Dataset<layers::Landing>, at: DateTime<Utc>) -> PathBuf {
        let path = dataset
            .clone()
            .on_date(at.date_naive())
            .unwrap()
            .batch_file(at);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::File::create(&path).unwrap();
        path
    }

    fn locations(files: &[LandedFile]) -> Vec<&ObjectPath> {
        files.iter().map(LandedFile::location).collect()
    }

    fn location_of(tmp: &tempfile::TempDir, path: &Path) -> ObjectPath {
        ObjectPath::from(
            path.strip_prefix(tmp.path())
                .unwrap()
                .to_str()
                .expect("a path in UTF-8"),
        )
    }

    #[tokio::test]
    async fn a_landing_dataset_nothing_has_written_holds_no_files() {
        let tmp = tempfile::tempdir().unwrap();

        let files = Root::new(tmp.path())
            .dataset(POLL)
            .landed_files()
            .await
            .unwrap();

        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn landed_files_are_listed_across_partitions_oldest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let dataset = Root::new(tmp.path()).dataset(POLL);
        let latest = land(
            &dataset,
            Utc.with_ymd_and_hms(2026, 10, 2, 0, 0, 0).unwrap(),
        );
        let middle = land(
            &dataset,
            Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).unwrap(),
        );
        let earliest = land(
            &dataset,
            Utc.with_ymd_and_hms(2026, 10, 1, 8, 0, 0).unwrap(),
        );

        let files = dataset.landed_files().await.unwrap();

        assert_eq!(
            locations(&files),
            [&earliest, &middle, &latest]
                .map(|path| location_of(&tmp, path))
                .each_ref()
        );
    }

    #[tokio::test]
    async fn a_file_still_being_written_is_not_listed() {
        let tmp = tempfile::tempdir().unwrap();
        let dataset = Root::new(tmp.path()).dataset(POLL);
        let finished = land(
            &dataset,
            Utc.with_ymd_and_hms(2026, 10, 1, 8, 0, 0).unwrap(),
        );
        let staged = format!("{}#0", finished.display());
        std::fs::File::create(staged).unwrap();

        let files = dataset.landed_files().await.unwrap();

        assert_eq!(locations(&files), [&location_of(&tmp, &finished)]);
    }

    #[tokio::test]
    async fn deleting_a_landed_file_leaves_the_others() {
        let tmp = tempfile::tempdir().unwrap();
        let dataset = Root::new(tmp.path()).dataset(POLL);
        let first = land(
            &dataset,
            Utc.with_ymd_and_hms(2026, 10, 1, 8, 0, 0).unwrap(),
        );
        let second = land(
            &dataset,
            Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).unwrap(),
        );
        let drained = dataset.landed_files().await.unwrap().remove(0);

        drained.delete().await.unwrap();

        assert!(!first.exists());
        assert_eq!(
            locations(&dataset.landed_files().await.unwrap()),
            [&location_of(&tmp, &second)]
        );
    }

    #[tokio::test]
    async fn deleting_a_file_already_gone_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let dataset = Root::new(tmp.path()).dataset(POLL);
        let path = land(
            &dataset,
            Utc.with_ymd_and_hms(2026, 10, 1, 8, 0, 0).unwrap(),
        );
        let file = dataset.landed_files().await.unwrap().remove(0);
        std::fs::remove_file(path).unwrap();

        let err = file.delete().await.unwrap_err();

        assert!(matches!(err, LandingError::Delete { .. }), "{err}");
    }

    #[test]
    fn an_unpartitioned_dataset_is_root_layer_name() {
        let dir = root().dataset(EXTRACT_MANIFEST).dir();

        assert_eq!(dir.to_str().unwrap(), "/store/bronze/extract_manifest");
    }

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
    fn a_gold_artefact_is_named_by_what_it_is_and_versioned_by_its_run() {
        let root = Root::new("/store");
        let run = |minute| Utc.with_ymd_and_hms(2026, 8, 1, 19, minute, 57).unwrap();

        let first = root
            .gold_artefact("crossings", run(48), "crossings.pointset")
            .unwrap();
        let second = root
            .gold_artefact("crossings", run(52), "crossings.pointset")
            .unwrap();

        assert_eq!(
            first.to_str().unwrap(),
            "/store/gold/artifact=crossings/version=20260801T194857000Z/crossings.pointset"
        );
        assert_ne!(first, second);
    }

    #[test]
    fn a_gold_artefact_whose_name_could_not_be_a_partition_is_refused() {
        let root = Root::new("/store");
        let run = Utc.with_ymd_and_hms(2026, 8, 1, 19, 48, 57).unwrap();

        assert!(
            root.gold_artefact("water/crossings", run, "buffer")
                .is_err()
        );
        assert!(root.gold_artefact("", run, "buffer").is_err());
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

    #[test]
    fn the_workspace_is_found_by_walking_up_from_the_working_directory() {
        let workspace = workspace_root().expect("locate the workspace");

        assert!(workspace.join("Cargo.toml").exists());
        assert!(
            std::fs::read_to_string(workspace.join("Cargo.toml"))
                .expect("read the manifest")
                .contains(WORKSPACE_SECTION)
        );
    }
}
