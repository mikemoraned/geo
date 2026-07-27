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

use geo_types::{MultiPolygon, Rect};
use wkt::ToWkt;

/// Default Overture release to read. Overture publishes monthly and only keeps the
/// most recent releases on S3, so this needs bumping as old ones age out; override
/// with `--release`.
pub const DEFAULT_RELEASE: &str = "2026-06-17.0";

/// The public Overture bucket's region; the bucket name embeds it too.
const S3_REGION: &str = "us-west-2";

/// Rail `class`es dropped when extracting: street `tram` lines aren't the transport
/// we care about. Applied to both the segment fetch and the connector-reference
/// subquery, so tram connectors are excluded along with tram segments.
pub(crate) const EXCLUDED_CLASSES: &[&str] = &["tram"];

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

    /// Query every **rail** `segment` whose geometry intersects any of `bboxes`,
    /// returning `id`/`subtype`/`class`, the geometry as WKB (`ST_AsBinary`), and the
    /// segment's bounding box flattened from Overture's `bbox` struct to
    /// `min_lon`/`max_lon`/`min_lat`/`max_lat`. Requires [`register_segments`] first.
    /// One query against the union of the bbox envelopes, so the partition is scanned
    /// once and SedonaDB prunes row groups by the combined bbox covering; the
    /// `subtype = 'rail'` filter keeps only rail. Empty `bboxes` yields no rows without
    /// touching S3.
    pub async fn rail_segments(
        &self,
        bboxes: &[Rect<f64>],
    ) -> Result<Vec<RecordBatch>, OvertureError> {
        if bboxes.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT id, subtype, class, ST_AsBinary(geometry) AS geometry,
                    bbox['xmin'] AS min_lon, bbox['xmax'] AS max_lon,
                    bbox['ymin'] AS min_lat, bbox['ymax'] AS max_lat
             FROM segments
             WHERE subtype = 'rail'
               AND {class}
               AND ST_Intersects(geometry, ST_SetSRID(ST_GeomFromWKT('{window}'), 4326))",
            class = class_filter("class"),
            window = bboxes_multipolygon_wkt(bboxes),
        );
        Ok(self.ctx.sql(&sql).await?.collect().await?)
    }

    /// Fetch the `connectors` referenced by the rail segments intersecting `bboxes`,
    /// returning `id`, geometry as WKB, and the connector's bounding box flattened from
    /// Overture's `bbox` struct. Requires both [`register_segments`] and
    /// [`register_connectors`]. A connector is kept when its point falls in the same
    /// window (so the connector partition is pruned by its bbox covering) *and* its
    /// `id` is one of the `connector_id`s referenced by a rail segment in the window.
    /// The connector table has unique `id`s, so the result is already deduped.
    ///
    /// Caveat: the spatial predicate drops the occasional referenced connector that
    /// sits just outside the window (an endpoint of a rail segment that only clips the
    /// box).
    pub async fn rail_connectors(
        &self,
        bboxes: &[Rect<f64>],
    ) -> Result<Vec<RecordBatch>, OvertureError> {
        if bboxes.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT c.id, ST_AsBinary(c.geometry) AS geometry,
                    c.bbox['xmin'] AS min_lon, c.bbox['xmax'] AS max_lon,
                    c.bbox['ymin'] AS min_lat, c.bbox['ymax'] AS max_lat
             FROM connectors AS c
             WHERE ST_Intersects(c.geometry, ST_SetSRID(ST_GeomFromWKT('{window}'), 4326))
               AND c.id IN (
                 SELECT DISTINCT elem['connector_id']
                 FROM (
                   SELECT UNNEST(s.connectors) AS elem
                   FROM segments AS s
                   WHERE s.subtype = 'rail'
                     AND {class}
                     AND ST_Intersects(s.geometry, ST_SetSRID(ST_GeomFromWKT('{window}'), 4326))
                 ) AS refs
               )",
            class = class_filter("s.class"),
            window = bboxes_multipolygon_wkt(bboxes),
        );
        Ok(self.ctx.sql(&sql).await?.collect().await?)
    }
}

/// A MULTIPOLYGON WKT covering every bbox, for use as a single spatial query window
/// over all of them at once. Each [`Rect`] becomes a closed rectangular polygon (`lon
/// lat`, x-y). Caller ensures `bboxes` is non-empty.
fn bboxes_multipolygon_wkt(bboxes: &[Rect<f64>]) -> String {
    let polygons = bboxes.iter().copied().map(Rect::to_polygon).collect();
    MultiPolygon::new(polygons).wkt_string()
}

/// A SQL predicate excluding [`EXCLUDED_CLASSES`] on `column`, keeping null-class rows
/// (`coalesce` maps a null class to `''`, which is never an excluded class). `TRUE`
/// when nothing is excluded, so it composes into an `AND` chain unconditionally.
fn class_filter(column: &str) -> String {
    if EXCLUDED_CLASSES.is_empty() {
        return "TRUE".to_string();
    }
    let excluded = EXCLUDED_CLASSES
        .iter()
        .map(|class| format!("'{class}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("coalesce({column}, '') NOT IN ({excluded})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo_types::Coord;

    fn bbox(min_lat: f64, max_lat: f64, min_lon: f64, max_lon: f64) -> Rect<f64> {
        Rect::new(
            Coord {
                x: min_lon,
                y: min_lat,
            },
            Coord {
                x: max_lon,
                y: max_lat,
            },
        )
    }

    #[test]
    fn multipolygon_covers_every_bbox_as_a_closed_ring() {
        let wkt =
            bboxes_multipolygon_wkt(&[bbox(50.0, 51.0, 11.0, 12.0), bbox(52.0, 53.0, 13.0, 14.0)]);
        assert_eq!(
            wkt,
            "MULTIPOLYGON(((12 50,12 51,11 51,11 50,12 50)),((14 52,14 53,13 53,13 52,14 52)))",
            "one closed lon-lat rectangle ring per bbox"
        );
    }

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

    /// The class filter excludes each configured class on the given column while a
    /// `coalesce` keeps null-class rows.
    #[test]
    fn class_filter_excludes_configured_classes_keeping_nulls() {
        assert!(
            !EXCLUDED_CLASSES.is_empty(),
            "expects at least one exclusion"
        );
        let filter = class_filter("s.class");
        assert!(filter.starts_with("coalesce(s.class, '') NOT IN ("));
        for class in EXCLUDED_CLASSES {
            assert!(filter.contains(&format!("'{class}'")));
        }
    }
}
