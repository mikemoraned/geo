use std::collections::HashMap;
use std::path::{Path, PathBuf};

use arrow::array::RecordBatch;
use datafusion::error::DataFusionError;
use datafusion::execution::SendableRecordBatchStream;
use object_store::{ObjectStore, aws::AmazonS3Builder, path::Path as ObjectPath};
use sedona::context::SedonaContext;
use sedona_geoparquet::provider::GeoParquetReadOptions;

pub const DEFAULT_RELEASE: &str = "2026-06-17.0";

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
        release: String,
        location: String,
        available: Vec<String>,
    },
    #[error("no mirror at {root}")]
    MirrorAbsent { root: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    id: String,
    mirror: Option<PathBuf>,
}

impl Release {
    pub fn published(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            mirror: None,
        }
    }

    pub fn mirrored(id: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            id: id.into(),
            mirror: Some(path.into()),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    fn location(&self) -> String {
        match &self.mirror {
            Some(root) => root.display().to_string(),
            None => format!("s3://overturemaps-{S3_REGION}/{RELEASE_PREFIX}"),
        }
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
    let available = match &release.mirror {
        Some(root) if !root.is_dir() => {
            return OvertureError::MirrorAbsent {
                root: root.display().to_string(),
            };
        }
        Some(root) => mirrored_releases(root),
        None => match published_releases().await {
            Ok(releases) => releases,
            Err(failure) => {
                tracing::warn!(error = %failure, "could not list the published releases");
                return error.into();
            }
        },
    };
    if available.contains(&release.id) {
        error.into()
    } else {
        OvertureError::ReleaseAbsent {
            release: release.id.clone(),
            location: release.location(),
            available,
        }
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
            release.id(),
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

    #[test]
    fn published_read_options_are_valid() {
        Release::published("2025-08-20.0")
            .read_options()
            .expect("valid anonymous S3 options");
    }
}
