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

    /// Query every **rail** `segment` whose geometry intersects any of `bboxes`,
    /// returning `id`/`subtype`/`class`, the geometry as WKB (`ST_AsBinary`), and the
    /// referenced `connectors` (a list of `{connector_id, at}`). Requires
    /// [`register_segments`] first. One query against the union of the bbox envelopes,
    /// so the partition is scanned once and SedonaDB prunes row groups by the combined
    /// bbox covering; the `subtype = 'rail'` filter keeps only rail. Empty `bboxes`
    /// yields no rows without touching S3.
    pub async fn rail_segments(&self, bboxes: &[BBox]) -> Result<Vec<RecordBatch>, OvertureError> {
        if bboxes.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT id, subtype, class, ST_AsBinary(geometry) AS geometry, connectors
             FROM segments
             WHERE subtype = 'rail'
               AND ST_Intersects(geometry, ST_SetSRID(ST_GeomFromWKT('{window}'), 4326))",
            window = bboxes_multipolygon_wkt(bboxes),
        );
        Ok(self.ctx.sql(&sql).await?.collect().await?)
    }
}

/// A closed rectangular ring for `bbox` in WKT `lon lat` (x-y) order —
/// `((w s, e s, e n, w n, w s))` — the inner form shared by POLYGON/MULTIPOLYGON.
fn bbox_ring_wkt(bbox: &BBox) -> String {
    let (w, e, s, n) = (bbox.min_lon, bbox.max_lon, bbox.min_lat, bbox.max_lat);
    format!("(({w} {s}, {e} {s}, {e} {n}, {w} {n}, {w} {s}))")
}

/// A MULTIPOLYGON WKT covering every bbox, for use as a single spatial query window
/// over all of them at once. Caller ensures `bboxes` is non-empty.
fn bboxes_multipolygon_wkt(bboxes: &[BBox]) -> String {
    let rings: Vec<String> = bboxes.iter().map(bbox_ring_wkt).collect();
    format!("MULTIPOLYGON({})", rings.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bbox(min_lat: f64, max_lat: f64, min_lon: f64, max_lon: f64) -> BBox {
        BBox {
            min_lat,
            max_lat,
            min_lon,
            max_lon,
        }
    }

    #[test]
    fn multipolygon_covers_every_bbox_as_a_closed_ring() {
        let wkt =
            bboxes_multipolygon_wkt(&[bbox(50.0, 51.0, 11.0, 12.0), bbox(52.0, 53.0, 13.0, 14.0)]);
        assert_eq!(
            wkt,
            "MULTIPOLYGON(((11 50, 12 50, 12 51, 11 51, 11 50)), ((13 52, 14 52, 14 53, 13 53, 13 52)))",
            "one closed lon-lat ring per bbox, SW corner first"
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
