use std::collections::HashMap;
use std::path::{Path, PathBuf};

use arrow::array::RecordBatch;
use datafusion::error::DataFusionError;
use datafusion::execution::SendableRecordBatchStream;
use medallion_model::OvertureRelease;
use object_store::{ObjectStore, aws::AmazonS3Builder, path::Path as ObjectPath};
use sedona::context::SedonaContext;
use sedona_geoparquet::provider::GeoParquetReadOptions;

const S3_REGION: &str = "us-west-2";

const RELEASE_PREFIX: &str = "release";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OvertureType {
    pub theme: &'static str,
    pub name: &'static str,
}

impl OvertureType {
    const fn new(theme: &'static str, name: &'static str) -> Self {
        Self { theme, name }
    }

    pub const SEGMENT: Self = Self::new("transportation", "segment");
    pub const CONNECTOR: Self = Self::new("transportation", "connector");
    pub const WATER: Self = Self::new("base", "water");
    pub const DIVISION_AREA: Self = Self::new("divisions", "division_area");
    pub const DIVISION: Self = Self::new("divisions", "division");
}

#[derive(Debug, thiserror::Error)]
pub enum OvertureError {
    #[error("datafusion error: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),
    #[error("invalid S3 read options: {0}")]
    ReadOptions(String),
    #[error(
        "release {release} is not in {location}, which holds {}",
        if available.is_empty() { "no releases".to_string() } else { available.join(", ") }
    )]
    ReleaseAbsent {
        release: OvertureRelease,
        location: String,
        available: Vec<String>,
    },
    #[error("no mirror at {root}")]
    MirrorAbsent { root: String },
    #[error("{location} holds no releases")]
    NoRelease { location: String },
    #[error("could not list the releases: {0}")]
    Listing(#[from] object_store::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseChoice {
    LatestReleaseAvailable,
    NamedRelease(OvertureRelease),
}

impl From<Option<OvertureRelease>> for ReleaseChoice {
    fn from(release: Option<OvertureRelease>) -> Self {
        release.map_or(Self::LatestReleaseAvailable, Self::NamedRelease)
    }
}

impl ReleaseChoice {
    pub async fn resolve(self, mirror: Option<PathBuf>) -> Result<Release, OvertureError> {
        match self {
            Self::NamedRelease(release) => Ok(Release::at(release, mirror)),
            Self::LatestReleaseAvailable => {
                let latest = available_releases(mirror.as_deref())
                    .await?
                    .iter()
                    .filter_map(|name| name.parse::<OvertureRelease>().ok())
                    .max();
                match latest {
                    Some(release) => Ok(Release::at(release, mirror)),
                    None => Err(OvertureError::NoRelease {
                        location: location(mirror.as_deref()),
                    }),
                }
            }
        }
    }
}

fn location(mirror: Option<&Path>) -> String {
    match mirror {
        Some(root) => root.display().to_string(),
        None => format!("s3://overturemaps-{S3_REGION}/{RELEASE_PREFIX}"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    id: OvertureRelease,
    mirror: Option<PathBuf>,
}

impl Release {
    #[cfg(test)]
    pub(crate) fn published(id: &str) -> Self {
        Self {
            id: id.parse().expect("a test names a release Overture would"),
            mirror: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn mirrored(id: &str, path: impl Into<PathBuf>) -> Self {
        Self {
            id: id.parse().expect("a test names a release Overture would"),
            mirror: Some(path.into()),
        }
    }

    pub fn at(id: OvertureRelease, mirror: Option<PathBuf>) -> Self {
        Self { id, mirror }
    }

    pub fn id(&self) -> OvertureRelease {
        self.id
    }

    pub fn location(&self) -> String {
        location(self.mirror.as_deref())
    }

    fn path(&self, overture_type: OvertureType) -> String {
        let OvertureType { theme, name } = overture_type;
        format!("{}/{}/theme={theme}/type={name}/", self.location(), self.id)
    }

    fn read_options(&self) -> Result<GeoParquetReadOptions<'static>, OvertureError> {
        if self.mirror.is_some() {
            return Ok(GeoParquetReadOptions::default());
        }
        let options = HashMap::from([
            ("aws.skip_signature".to_string(), "true".to_string()),
            ("aws.region".to_string(), S3_REGION.to_string()),
        ]);
        GeoParquetReadOptions::from_table_options(options).map_err(OvertureError::ReadOptions)
    }
}

async fn diagnose(release: &Release, error: DataFusionError) -> OvertureError {
    let available = match available_releases(release.mirror.as_deref()).await {
        Ok(releases) => releases,
        Err(OvertureError::Listing(failure)) => {
            tracing::warn!(error = %failure, "could not list the published releases");
            return error.into();
        }
        Err(absent) => return absent,
    };
    if available.contains(&release.id.to_string()) {
        error.into()
    } else {
        OvertureError::ReleaseAbsent {
            release: release.id,
            location: release.location(),
            available,
        }
    }
}

async fn available_releases(mirror: Option<&Path>) -> Result<Vec<String>, OvertureError> {
    match mirror {
        Some(root) if !root.is_dir() => Err(OvertureError::MirrorAbsent {
            root: root.display().to_string(),
        }),
        Some(root) => Ok(mirrored_releases(root)),
        None => Ok(published_releases().await?),
    }
}

fn mirrored_releases(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut releases: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    releases.sort();
    releases
}

async fn published_releases() -> Result<Vec<String>, object_store::Error> {
    let bucket = AmazonS3Builder::new()
        .with_bucket_name(format!("overturemaps-{S3_REGION}"))
        .with_region(S3_REGION)
        .with_skip_signature(true)
        .build()?;
    let listing = bucket
        .list_with_delimiter(Some(&ObjectPath::from(RELEASE_PREFIX)))
        .await?;
    let mut releases: Vec<String> = listing
        .common_prefixes
        .iter()
        .filter_map(|release| release.filename().map(String::from))
        .collect();
    releases.sort();
    Ok(releases)
}

pub struct Overture {
    ctx: SedonaContext,
    release: Release,
}

impl Overture {
    pub fn open(release: Release) -> Self {
        Self {
            ctx: SedonaContext::new(),
            release,
        }
    }

    pub fn release(&self) -> &Release {
        &self.release
    }

    pub async fn register(
        &self,
        overture_type: OvertureType,
        table: &str,
    ) -> Result<(), OvertureError> {
        let df = match self
            .ctx
            .read_parquet(
                self.release.path(overture_type),
                self.release.read_options()?,
            )
            .await
        {
            Ok(df) => df,
            Err(error) => return Err(diagnose(&self.release, error).await),
        };
        self.ctx.ctx.register_table(table, df.into_view())?;
        Ok(())
    }

    pub async fn sql(&self, sql: &str) -> Result<Vec<RecordBatch>, OvertureError> {
        Ok(self.ctx.sql(sql).await?.collect().await?)
    }

    pub async fn stream(&self, sql: &str) -> Result<SendableRecordBatchStream, OvertureError> {
        Ok(self.ctx.sql(sql).await?.execute_stream().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_published_release_reads_from_the_public_bucket() {
        let release = Release::published("2025-08-20.0");

        assert_eq!(
            release.path(OvertureType::SEGMENT),
            "s3://overturemaps-us-west-2/release/2025-08-20.0/theme=transportation/type=segment/"
        );
    }

    #[test]
    fn a_mirrored_release_reads_the_same_layout_from_disk() {
        let release = Release::mirrored("2025-08-20.0", "/mirror/release");

        assert_eq!(
            release.path(OvertureType::WATER),
            "/mirror/release/2025-08-20.0/theme=base/type=water/"
        );
        assert_eq!(
            release.id().to_string(),
            "2025-08-20.0",
            "the id is the release, not the path"
        );
    }

    #[tokio::test]
    async fn a_read_from_an_unloaded_mirror_reports_the_mirror_as_absent() {
        let release = Release::mirrored("2025-08-20.0", "/no/such/mirror");

        let error = diagnose(&release, DataFusionError::Plan("zero objects".into())).await;

        assert!(
            matches!(error, OvertureError::MirrorAbsent { .. }),
            "expected an absent mirror, got {error}"
        );
    }

    #[tokio::test]
    async fn a_read_of_a_release_a_mirror_lacks_names_the_releases_it_holds() {
        let mirror = tempfile::tempdir().expect("a temp dir");
        std::fs::create_dir(mirror.path().join("2025-09-24.0")).expect("a mirrored release");
        let release = Release::mirrored("2025-08-20.0", mirror.path());

        let error = diagnose(&release, DataFusionError::Plan("zero objects".into())).await;

        let OvertureError::ReleaseAbsent { available, .. } = &error else {
            panic!("expected an absent release, got {error}");
        };
        assert_eq!(available, &["2025-09-24.0".to_string()]);
    }

    #[tokio::test]
    async fn a_read_of_a_release_a_mirror_holds_reports_the_underlying_failure() {
        let mirror = tempfile::tempdir().expect("a temp dir");
        std::fs::create_dir(mirror.path().join("2025-08-20.0")).expect("a mirrored release");
        let release = Release::mirrored("2025-08-20.0", mirror.path());

        let error = diagnose(&release, DataFusionError::Plan("zero objects".into())).await;

        assert!(
            matches!(error, OvertureError::DataFusion(_)),
            "expected the read's own failure, got {error}"
        );
    }

    fn mirror_holding(names: &[&str]) -> tempfile::TempDir {
        let mirror = tempfile::tempdir().expect("a temp dir");
        for name in names {
            std::fs::create_dir(mirror.path().join(name)).expect("a mirrored directory");
        }
        mirror
    }

    #[tokio::test]
    async fn the_latest_release_available_is_the_newest_a_mirror_holds() {
        let mirror = mirror_holding(&["2026-08-19.0", "2026-09-23.1", "2026-09-23.0"]);

        let release = ReleaseChoice::LatestReleaseAvailable
            .resolve(Some(mirror.path().into()))
            .await
            .expect("a release");

        assert_eq!(release, Release::mirrored("2026-09-23.1", mirror.path()));
    }

    #[tokio::test]
    async fn the_latest_release_available_orders_reissues_by_number() {
        let mirror = mirror_holding(&["2026-09-23.2", "2026-09-23.10"]);

        let release = ReleaseChoice::LatestReleaseAvailable
            .resolve(Some(mirror.path().into()))
            .await
            .expect("a release");

        assert_eq!(release.id().to_string(), "2026-09-23.10");
    }

    #[tokio::test]
    async fn the_latest_release_available_ignores_directories_that_are_not_releases() {
        let mirror = mirror_holding(&["2026-09-23.1", "@eaDir", "#recycle", ".Trashes"]);

        let release = ReleaseChoice::LatestReleaseAvailable
            .resolve(Some(mirror.path().into()))
            .await
            .expect("a release");

        assert_eq!(release.id().to_string(), "2026-09-23.1");
    }

    #[tokio::test]
    async fn a_mirror_without_releases_has_no_latest_release() {
        let mirror = mirror_holding(&["@eaDir"]);

        let error = ReleaseChoice::LatestReleaseAvailable
            .resolve(Some(mirror.path().into()))
            .await
            .expect_err("no release");

        assert!(
            matches!(error, OvertureError::NoRelease { .. }),
            "expected no release, got {error}"
        );
    }

    #[tokio::test]
    async fn an_absent_mirror_has_no_latest_release() {
        let error = ReleaseChoice::LatestReleaseAvailable
            .resolve(Some("/no/such/mirror".into()))
            .await
            .expect_err("no mirror");

        assert!(
            matches!(error, OvertureError::MirrorAbsent { .. }),
            "expected an absent mirror, got {error}"
        );
    }

    #[tokio::test]
    async fn a_named_release_resolves_without_reading_the_mirror() {
        let release = ReleaseChoice::NamedRelease("2026-07-22.0".parse().expect("a release"))
            .resolve(Some("/no/such/mirror".into()))
            .await
            .expect("a release");

        assert_eq!(
            release,
            Release::mirrored("2026-07-22.0", "/no/such/mirror")
        );
    }

    #[test]
    fn published_read_options_are_valid() {
        Release::published("2025-08-20.0")
            .read_options()
            .expect("valid anonymous S3 options");
    }
}
