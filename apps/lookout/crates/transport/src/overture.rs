//! Live access to Overture Maps transportation data via SedonaDB, queried in-process
//! against the public S3 bucket. Opens a SedonaDB context, registers the release's
//! `theme=transportation` GeoParquet as tables (read anonymously from S3), and runs
//! bbox-filtered queries over them.

use std::collections::HashMap;

use arrow::array::RecordBatch;
use sedona::context::SedonaContext;
use sedona_geoparquet::provider::GeoParquetReadOptions;

use crate::groups::BBox;

/// Default Overture release to read. Overture publishes monthly and only keeps the
/// most recent releases on S3, so this needs bumping as old ones age out; override
/// with `--release`.
pub const DEFAULT_RELEASE: &str = "2026-06-17.0";

/// The public Overture bucket's region; the bucket name embeds it too.
const S3_REGION: &str = "us-west-2";

/// Failure opening or querying Overture.
#[derive(Debug, thiserror::Error)]
pub enum OvertureError {
    #[error("datafusion error: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),
    #[error("invalid S3 read options: {0}")]
    ReadOptions(String),
}

/// A SedonaDB context pointed at one Overture release on S3.
pub struct Overture {
    ctx: SedonaContext,
    release: String,
}

impl Overture {
    /// Open a SedonaDB context for `release`. No network happens until a table is
    /// registered or queried.
    pub fn open(release: impl Into<String>) -> Self {
        Self {
            ctx: SedonaContext::new(),
            release: release.into(),
        }
    }

    /// The S3 directory prefix for one `theme=transportation` type (`segment` |
    /// `connector`). A trailing slash so the reader lists the partition's `.parquet`
    /// files (a duckdb-style `/*` glob fails DataFusion's `.parquet` extension check).
    fn transportation_path(&self, overture_type: &str) -> String {
        format!(
            "s3://overturemaps-{S3_REGION}/release/{}/theme=transportation/type={overture_type}/",
            self.release
        )
    }

    /// GeoParquet read options for anonymous access to the public bucket: unsigned
    /// requests (`aws.skip_signature`) against `us-west-2`.
    fn s3_read_options() -> Result<GeoParquetReadOptions<'static>, OvertureError> {
        let options = HashMap::from([
            ("aws.skip_signature".to_string(), "true".to_string()),
            ("aws.region".to_string(), S3_REGION.to_string()),
        ]);
        GeoParquetReadOptions::from_table_options(options).map_err(OvertureError::ReadOptions)
    }

    /// Register the release's `segment` GeoParquet as a queryable table `segments`,
    /// read anonymously from S3. Replaces any existing registration of that name.
    pub async fn register_segments(&self) -> Result<(), OvertureError> {
        let df = self
            .ctx
            .read_parquet(
                self.transportation_path("segment"),
                Self::s3_read_options()?,
            )
            .await?;
        self.ctx.ctx.register_table("segments", df.into_view())?;
        Ok(())
    }

    /// Query up to `limit` `segments` rows whose geometry intersects `bbox`, returning
    /// `id`/`subtype`/`class`. Requires [`register_segments`] first. Uses a spatial
    /// predicate against the bbox envelope so SedonaDB prunes GeoParquet row groups by
    /// their bbox covering — without it, the query scans the whole global partition.
    pub async fn segments_in_bbox(
        &self,
        bbox: &BBox,
        limit: usize,
    ) -> Result<Vec<RecordBatch>, OvertureError> {
        let sql = format!(
            "SELECT id, subtype, class FROM segments
             WHERE ST_Intersects(geometry, ST_SetSRID(ST_GeomFromWKT('{envelope}'), 4326))
             LIMIT {limit}",
            envelope = bbox_envelope_wkt(bbox),
        );
        Ok(self.ctx.sql(&sql).await?.collect().await?)
    }
}

/// A closed rectangular ring (POLYGON WKT) around `bbox`, with coordinates written as
/// `lon lat` (WKT/x-y order), for use as a spatial query window.
fn bbox_envelope_wkt(bbox: &BBox) -> String {
    let (w, e, s, n) = (bbox.min_lon, bbox.max_lon, bbox.min_lat, bbox.max_lat);
    format!("POLYGON(({w} {s}, {e} {s}, {e} {n}, {w} {n}, {w} {s}))")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbox_envelope_is_a_closed_lon_lat_ring() {
        let wkt = bbox_envelope_wkt(&BBox {
            min_lat: 50.0,
            max_lat: 51.0,
            min_lon: 11.0,
            max_lon: 12.0,
        });
        assert_eq!(
            wkt, "POLYGON((11 50, 12 50, 12 51, 11 51, 11 50))",
            "corners run anticlockwise from the SW and close back to it"
        );
    }

    /// The S3 glob embeds the release and the transportation type, against the public
    /// `us-west-2` bucket. (A pure string check — no network.)
    #[test]
    fn transportation_path_targets_the_release_partition() {
        let overture = Overture::open("2025-08-20.0");
        assert_eq!(
            overture.transportation_path("segment"),
            "s3://overturemaps-us-west-2/release/2025-08-20.0/theme=transportation/type=segment/"
        );
    }

    /// The anonymous read options are accepted (the option names are validated by
    /// `from_table_options`, which errors on typos).
    #[test]
    fn s3_read_options_are_valid() {
        Overture::s3_read_options().expect("valid anonymous S3 options");
    }
}
