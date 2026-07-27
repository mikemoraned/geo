//! Derive the silver `train_segment` dataset from the bronze capture log: collapse the
//! duplication-allowed rows down to one per scheduled leg (newest capture wins, so its
//! realtime-corrected times survive), decode each Google polyline to a lat/lon line, and
//! store it as WKB alongside the same line projected into metres.
//!
//! Silver holds one current row per leg, so a run rewrites each `departure_date` partition
//! it touches: re-running over unchanged bronze produces an identical dataset.

use std::sync::Arc;

use arrow::array::{ArrayRef, RecordBatch};
use arrow::datatypes::{FieldRef, Schema};
use chrono::{DateTime, Utc};
use geo_types::LineString;
use medallion::{Country, Projector, Query, Root};
use serde::{Deserialize, Serialize};

/// Precision the Motis `map/trips` polylines are encoded at.
const POLYLINE_PRECISION: u32 = 5;

/// Lat/lon geometry, in the global CRS every silver dataset carries.
const GEOMETRY: &str = "geometry";

/// The same geometry in metres, for distance and length work.
const PROJECTED_GEOMETRY: &str = "geometry_projected";

/// The encoded geometry a leg arrives with, which the geometry columns replace.
const POLYLINE: &str = "polyline";

/// The country these segments run in, which fixes the projected geometry's CRS.
const COUNTRY: Country = Country::Germany;

/// Instant columns, declared rather than traced — see [`crate::bronze`].
const INSTANT_COLUMNS: [&str; 2] = ["departure", "arrival"];

/// The capture log under its query name.
const CAPTURED: &str = "captured";

/// The deduped legs, materialised so each partition's rows are a scan of them rather than
/// a fresh dedup of the whole capture log.
const DEDUPED_LEGS: &str = "deduped";

/// One row per scheduled leg, newest capture kept.
///
/// A leg's identity is `(trip_id, from_stop_id, departure)`: `departure` alone is not
/// unique per trip, since minute-resolution timetables let two legs of one trip depart
/// different stops in the same minute.
const DEDUPED: &str = "
    SELECT * EXCLUDE (rank)
    FROM (
      SELECT *, ROW_NUMBER() OVER (
        PARTITION BY trip_id, from_stop_id, departure ORDER BY captured_at DESC
      ) AS rank
      FROM captured
    )
    WHERE rank = 1
";

/// What one ingest run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IngestOutcome {
    /// Rows read from the capture log.
    pub read: usize,
    /// Distinct legs after dedup, which is also the rows written.
    pub deduped: usize,
    /// Partitions rewritten.
    pub partitions: usize,
}

/// A failure deriving the dataset.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("querying the capture log: {0}")]
    Query(#[from] medallion::QueryError),
    #[error("decoding polyline: {0}")]
    Polyline(String),
    #[error("encoding wkb: {0}")]
    Wkb(#[from] wkb::error::WkbError),
    #[error("building the record batch: {0}")]
    Encode(#[from] serde_arrow::Error),
    #[error("describing the rows: {0}")]
    Rows(#[from] medallion::RowError),
    #[error("assembling the batch: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    #[error("geometry: {0}")]
    Geo(#[from] medallion::GeoError),
    #[error("partitioning the dataset: {0}")]
    Path(#[from] medallion::PathError),
    #[error("writing the dataset: {0}")]
    Write(#[from] medallion::WriteError),
}

/// One deduped leg as the query returns it: the columns the silver dataset holds, plus the
/// still-encoded `polyline` the geometry columns are built from. The polyline itself is
/// dropped before writing, since the geometry columns replace it.
#[derive(Debug, Serialize, Deserialize)]
struct Leg {
    trip_id: String,
    route_name: Option<String>,
    train_number: Option<u32>,
    agency_id: Option<String>,
    agency_name: Option<String>,
    mode: String,
    route_color: Option<String>,
    realtime: bool,
    from_stop_id: Option<String>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    departure: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    arrival: DateTime<Utc>,
    polyline: String,
}

/// Derive the silver dataset from the bronze capture log in the same store.
///
/// Dedup and partitioning are one SQL query each against the capture log; the polyline
/// decoding and projection either query does not express happen per partition in Rust.
pub async fn ingest(root: &Root) -> Result<IngestOutcome, IngestError> {
    let query = Query::new(root.clone());
    if !query
        .register_if_present(model::MOTIS_SEGMENT, CAPTURED)
        .await?
    {
        return Ok(IngestOutcome::default());
    }

    let read = query
        .count("SELECT COUNT(*) AS count FROM captured")
        .await? as usize;

    // Dedup once into a table the per-partition queries read, rather than as a subquery
    // they each re-run: the window sorts the whole capture log, so repeating it per
    // partition would rescan all of history once per date.
    query
        .sql(&format!("CREATE TABLE {DEDUPED_LEGS} AS {DEDUPED}"))
        .await?;

    let legs: Vec<Leg> = query
        .rows(&format!(
            "SELECT trip_id, route_name, train_number, agency_id, agency_name, mode,
                    route_color, realtime, from_stop_id, departure, arrival, polyline
             FROM {DEDUPED_LEGS}
             ORDER BY departure"
        ))
        .await?;
    let deduped = legs.len();

    // Ordered by departure, so each partition's legs are one run of adjacent rows.
    let projector = Projector::for_country(COUNTRY)?;
    let mut partitions = 0;
    for legs in legs.chunk_by(|a, b| a.departure.date_naive() == b.departure.date_naive()) {
        let date = legs[0].departure.date_naive();
        root.dataset(model::TRAIN_SEGMENT)
            .on_date(date)?
            .rebuild_geo(&[batch(legs, &projector)?])
            .await?;
        partitions += 1;
    }

    Ok(IngestOutcome {
        read,
        deduped,
        partitions,
    })
}

/// Build one partition's batch: the columns the legs carry, then the two geometry columns
/// derived from their polylines.
fn batch(legs: &[Leg], projector: &Projector) -> Result<RecordBatch, IngestError> {
    let mut fields = medallion::fields::<Leg>(&INSTANT_COLUMNS)?;
    let mut arrays = serde_arrow::to_arrow(&fields, legs)?;
    drop_column(&mut fields, &mut arrays, POLYLINE);

    let lines = legs
        .iter()
        .map(|leg| decode_polyline(&leg.polyline))
        .collect::<Result<Vec<_>, _>>()?;
    let projected = lines
        .iter()
        .map(|line| projector.project(line))
        .collect::<Result<Vec<_>, _>>()?;

    for (field, geometries) in [
        (medallion::wkb_field(GEOMETRY)?, &lines),
        (
            medallion::projected_wkb_field(PROJECTED_GEOMETRY, COUNTRY)?,
            &projected,
        ),
    ] {
        let (field, array) = medallion::wkb_column(field, geometries)?;
        fields.push(field);
        arrays.push(array);
    }

    Ok(RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)?)
}

/// Remove `column` from a batch under construction, keeping fields and arrays aligned.
fn drop_column(fields: &mut Vec<FieldRef>, arrays: &mut Vec<ArrayRef>, column: &str) {
    if let Some(at) = fields.iter().position(|field| field.name() == column) {
        fields.remove(at);
        arrays.remove(at);
    }
}

/// Decode a Google-encoded polyline to a `(lon, lat)` line.
fn decode_polyline(encoded: &str) -> Result<LineString<f64>, IngestError> {
    polyline::decode_polyline(encoded, POLYLINE_PRECISION)
        .map_err(|e| IngestError::Polyline(e.to_string()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap as Map;

    use crate::bronze::SegmentLog;

    use chrono::TimeZone;
    use motis_openapi_progenitor::types::TripSegment;

    use super::*;

    fn fixture() -> Vec<TripSegment> {
        serde_json::from_str(include_str!("../tests/fixtures/trips.json")).expect("parse fixture")
    }

    /// Seed the capture log with a poll of the fixture segments at each instant, then
    /// derive from it.
    async fn ingest_polls(root: &Root, polls: &[DateTime<Utc>]) -> IngestOutcome {
        let log = SegmentLog::new(root.clone());
        for captured_at in polls {
            log.append(*captured_at, &fixture(), &Map::new())
                .await
                .expect("append poll");
        }
        ingest(root).await.expect("ingest")
    }

    /// The derived dataset, registered the way any other reader would register it.
    async fn derived(root: &Root) -> Query {
        let query = Query::new(root.clone());
        query
            .register(model::TRAIN_SEGMENT, "derived")
            .await
            .expect("register derived dataset");
        query
    }

    /// Both geometry columns of the derived dataset.
    async fn geometry_batches(root: &Root) -> Vec<RecordBatch> {
        derived(root)
            .await
            .sql(&format!(
                "SELECT ST_AsBinary({GEOMETRY}) AS {GEOMETRY},
                        ST_AsBinary({PROJECTED_GEOMETRY}) AS {PROJECTED_GEOMETRY}
                 FROM derived ORDER BY trip_id"
            ))
            .await
            .expect("query geometries")
    }

    /// The line held in `column` of the first row of `batch`.
    fn first_line(batch: &RecordBatch, column: &str) -> Vec<(f64, f64)> {
        let geometries = medallion::geometries(batch, column).expect("geometries");
        let geo_types::Geometry::LineString(line) = &geometries[0] else {
            panic!("{column} should hold a LineString");
        };
        line.coords().map(|c| (c.x, c.y)).collect()
    }

    #[tokio::test]
    async fn re_seen_legs_collapse_to_one_row_each() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let polls = [
            Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 30).unwrap(),
        ];

        let outcome = ingest_polls(&root, &polls).await;

        assert_eq!(outcome.read, fixture().len() * polls.len());
        assert_eq!(outcome.deduped, fixture().len());
    }

    #[tokio::test]
    async fn the_newest_capture_of_a_leg_wins() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let log = SegmentLog::new(root.clone());
        let mut later = fixture();
        for segment in &mut later {
            segment.real_time = !segment.real_time;
        }

        log.append(
            Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap(),
            &fixture(),
            &Map::new(),
        )
        .await
        .expect("first poll");
        log.append(
            Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 30).unwrap(),
            &later,
            &Map::new(),
        )
        .await
        .expect("second poll");
        ingest(&root).await.expect("ingest");

        let kept = derived(&root)
            .await
            .count(&format!(
                "SELECT COUNT(*) AS count FROM derived WHERE realtime = {}",
                later[0].real_time
            ))
            .await
            .expect("count");
        assert_eq!(
            kept,
            fixture().len() as i64,
            "every leg should carry the later capture's realtime flag"
        );
    }

    #[tokio::test]
    async fn each_leg_keeps_its_polyline_as_a_line_string() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let expected = polyline::decode_polyline(&fixture()[0].polyline, POLYLINE_PRECISION)
            .expect("decode")
            .0
            .len();

        ingest_polls(
            &root,
            &[Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap()],
        )
        .await;

        let batches = geometry_batches(&root).await;
        assert_eq!(first_line(&batches[0], GEOMETRY).len(), expected);
        assert_eq!(first_line(&batches[0], PROJECTED_GEOMETRY).len(), expected);
    }

    /// The lat/lon column holds degrees; the projected one holds metres in the German
    /// zone, whose eastings and northings are orders of magnitude larger.
    #[tokio::test]
    async fn the_projected_column_holds_metres_and_the_other_degrees() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());

        ingest_polls(
            &root,
            &[Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap()],
        )
        .await;

        let batches = geometry_batches(&root).await;
        let (lon, lat) = first_line(&batches[0], GEOMETRY)[0];
        let (easting, northing) = first_line(&batches[0], PROJECTED_GEOMETRY)[0];

        assert!(
            (-180.0..=180.0).contains(&lon) && (-90.0..=90.0).contains(&lat),
            "lat/lon out of range: {lon}, {lat}"
        );
        assert!(
            easting > 1_000.0 && northing > 1_000.0,
            "projected coordinates should be metres: {easting}, {northing}"
        );
    }

    /// Re-running over unchanged bronze rewrites the same partitions with the same rows.
    #[tokio::test]
    async fn re_ingesting_is_idempotent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());

        let first = ingest_polls(
            &root,
            &[Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap()],
        )
        .await;
        let rows_before = derived(&root)
            .await
            .count("SELECT COUNT(*) AS count FROM derived")
            .await
            .expect("count");
        let second = ingest(&root).await.expect("re-ingest");

        assert_eq!(first, second, "the same run, run twice");
        assert_eq!(
            derived(&root)
                .await
                .count("SELECT COUNT(*) AS count FROM derived")
                .await
                .expect("count"),
            rows_before,
            "re-running rewrites the partition rather than appending to it"
        );
    }

    #[tokio::test]
    async fn an_empty_capture_log_derives_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let outcome = ingest(&Root::new(tmp.path())).await.expect("ingest");

        assert_eq!(
            outcome,
            IngestOutcome {
                read: 0,
                deduped: 0,
                partitions: 0
            }
        );
    }
}
