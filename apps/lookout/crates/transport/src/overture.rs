//! Access to Overture Maps data via SedonaDB, queried in-process against one release —
//! either the public S3 bucket or a local mirror of it ([`Release`]). Opens a SedonaDB
//! context, registers a release's `theme=…/type=…` GeoParquet as tables, and runs queries
//! over them.

use std::collections::HashMap;
use std::path::PathBuf;

use arrow::array::RecordBatch;
use datafusion::execution::SendableRecordBatchStream;
use sedona::context::SedonaContext;
use sedona_geoparquet::provider::GeoParquetReadOptions;

/// Default Overture release to read. Overture publishes monthly and only keeps the
/// most recent releases on S3, so this needs bumping as old ones age out; override
/// with `--release`.
pub const DEFAULT_RELEASE: &str = "2026-06-17.0";

/// The public Overture bucket's region; the bucket name embeds it too.
const S3_REGION: &str = "us-west-2";

/// One `theme=…/type=…` partition of an Overture release — the unit a release is laid out
/// in, and so the unit an extract of it is read from and written back into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OvertureType {
    pub theme: &'static str,
    pub name: &'static str,
}

impl OvertureType {
    const fn new(theme: &'static str, name: &'static str) -> Self {
        Self { theme, name }
    }

    /// Roads, railways and the like; the rail subtype is what concerns us.
    pub const SEGMENT: Self = Self::new("transportation", "segment");
    /// The points segments join at.
    pub const CONNECTOR: Self = Self::new("transportation", "connector");
    /// Rivers, canals, lakes and coastline.
    pub const WATER: Self = Self::new("base", "water");
    /// Administrative boundaries as areas — where a country's own outline comes from.
    pub const DIVISION_AREA: Self = Self::new("divisions", "division_area");
    /// Administrative entities as points, localities among them.
    pub const DIVISION: Self = Self::new("divisions", "division");
}

/// Failure opening or querying Overture.
#[derive(Debug, thiserror::Error)]
pub enum OvertureError {
    #[error("datafusion error: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),
    #[error("invalid S3 read options: {0}")]
    ReadOptions(String),
}

/// Where a release is read from. A local mirror of the bucket holds the identical files
/// under the identical layout, so it is the same release by a shorter path — which is why
/// the id is recorded independently of the location, and provenance does not record which
/// of the two a given extraction happened to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    id: String,
    mirror: Option<PathBuf>,
}

impl Release {
    /// The release as published, read from the public S3 bucket.
    pub fn published(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            mirror: None,
        }
    }

    /// The same release read from a local mirror rooted at `path`, which contains the
    /// release's `theme=…` directories.
    pub fn mirrored(id: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            id: id.into(),
            mirror: Some(path.into()),
        }
    }

    /// The release identifier, as Overture publishes it.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The directory holding one type's parquet files. A trailing slash so the reader
    /// lists the partition's `.parquet` files (a duckdb-style `/*` glob fails
    /// DataFusion's `.parquet` extension check).
    fn path(&self, overture_type: OvertureType) -> String {
        let OvertureType { theme, name } = overture_type;
        match &self.mirror {
            Some(root) => format!("{}/theme={theme}/type={name}/", root.display()),
            None => format!(
                "s3://overturemaps-{S3_REGION}/release/{}/theme={theme}/type={name}/",
                self.id
            ),
        }
    }

    /// Read options for this location: anonymous, unsigned requests against the public
    /// bucket; nothing special for a mirror, which is just files on disk.
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

/// A SedonaDB context pointed at one Overture release.
pub struct Overture {
    ctx: SedonaContext,
    release: Release,
}

impl Overture {
    /// Open a SedonaDB context for `release`. Nothing is read until a table is registered
    /// or queried.
    pub fn open(release: Release) -> Self {
        Self {
            ctx: SedonaContext::new(),
            release,
        }
    }

    /// The release this reads.
    pub fn release(&self) -> &Release {
        &self.release
    }

    /// Register the release's `segment` GeoParquet as a queryable table `segments`.
    /// Replaces any existing registration of that name.
    pub async fn register_segments(&self) -> Result<(), OvertureError> {
        self.register(OvertureType::SEGMENT, "segments").await
    }

    /// Register the release's `connector` GeoParquet as a queryable table `connectors`.
    pub async fn register_connectors(&self) -> Result<(), OvertureError> {
        self.register(OvertureType::CONNECTOR, "connectors").await
    }

    /// Register one type's GeoParquet as a queryable table `table`. Replaces any
    /// existing registration of it.
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

    /// Run `sql` over the registered tables and collect the result.
    pub async fn sql(&self, sql: &str) -> Result<Vec<RecordBatch>, OvertureError> {
        Ok(self.ctx.sql(sql).await?.collect().await?)
    }

    /// Run `sql` over the registered tables, streaming the results rather than collecting
    /// them, for queries whose answer is too large to hold.
    pub async fn stream(&self, sql: &str) -> Result<SendableRecordBatchStream, OvertureError> {
        Ok(self.ctx.sql(sql).await?.execute_stream().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The S3 path embeds the release, theme and type, against the public `us-west-2`
    /// bucket. (A pure string check — nothing is read.)
    #[test]
    fn a_published_release_reads_from_the_public_bucket() {
        let release = Release::published("2025-08-20.0");

        assert_eq!(
            release.path(OvertureType::SEGMENT),
            "s3://overturemaps-us-west-2/release/2025-08-20.0/theme=transportation/type=segment/"
        );
    }

    /// A mirror is the same layout under a local root, so only the prefix differs.
    #[test]
    fn a_mirrored_release_reads_the_same_layout_from_disk() {
        let release = Release::mirrored("2025-08-20.0", "/mirror/2025-08-20.0");

        assert_eq!(
            release.path(OvertureType::WATER),
            "/mirror/2025-08-20.0/theme=base/type=water/"
        );
        assert_eq!(
            release.id(),
            "2025-08-20.0",
            "the id is the release, not the path"
        );
    }

    /// The anonymous read options are accepted (the option names are validated by
    /// `from_table_options`, which errors on typos).
    #[test]
    fn published_read_options_are_valid() {
        Release::published("2025-08-20.0")
            .read_options()
            .expect("valid anonymous S3 options");
    }
}
