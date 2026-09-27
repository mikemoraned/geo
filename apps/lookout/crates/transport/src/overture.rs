use std::collections::HashMap;
use std::path::PathBuf;

use arrow::array::RecordBatch;
use datafusion::execution::SendableRecordBatchStream;
use sedona::context::SedonaContext;
use sedona_geoparquet::provider::GeoParquetReadOptions;

pub const DEFAULT_RELEASE: &str = "2026-06-17.0";

const S3_REGION: &str = "us-west-2";

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

    fn path(&self, overture_type: OvertureType) -> String {
        let OvertureType { theme, name } = overture_type;
        match &self.mirror {
            Some(root) => format!("{}/{}/theme={theme}/type={name}/", root.display(), self.id),
            None => format!(
                "s3://overturemaps-{S3_REGION}/release/{}/theme={theme}/type={name}/",
                self.id
            ),
        }
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
        let df = self
            .ctx
            .read_parquet(
                self.release.path(overture_type),
                self.release.read_options()?,
            )
            .await?;
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

    #[test]
    fn published_read_options_are_valid() {
        Release::published("2025-08-20.0")
            .read_options()
            .expect("valid anonymous S3 options");
    }
}
