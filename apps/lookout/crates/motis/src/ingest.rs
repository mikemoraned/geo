//! Derive the silver `train_segment` dataset from the bronze capture log: collapse the
//! duplication-allowed rows down to one per scheduled leg (newest capture wins, so its
//! realtime-corrected times survive), decode each Google polyline to a lat/lon line, and
//! store it as WKB alongside the same line projected into metres.
//!
//! Silver holds one current row per leg, so a run rewrites each `departure_date` partition
//! it touches: re-running over unchanged bronze produces an identical dataset.

use std::sync::Arc;

use arrow::array::{ArrayRef, BinaryArray, RecordBatch};
use arrow::datatypes::{FieldRef, Schema};
use chrono::{DateTime, NaiveDate, Utc};
use geo_types::LineString;
use medallion::{Layer, Projector, Query, Root};
use serde::{Deserialize, Serialize};
use serde_arrow::schema::{SchemaLike, TracingOptions};
use serde_json::json;
use wkb::writer::{write_line_string, WriteOptions};

/// Precision the Motis `map/trips` polylines are encoded at.
const POLYLINE_PRECISION: u32 = 5;

/// The silver dataset this derives.
const DATASET: &str = "train_segment";

/// Lat/lon geometry, in the global CRS every silver dataset carries.
const GEOMETRY: &str = "geometry";

/// The same geometry in metres, for distance and length work.
const PROJECTED_GEOMETRY: &str = "geometry_projected";

/// Instant columns, declared rather than traced — see [`crate::bronze`].
const INSTANT_COLUMNS: [&str; 2] = ["departure", "arrival"];

/// The capture log under its query name.
const CAPTURED: &str = "captured";

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
    #[error("assembling the batch: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    #[error("geometry: {0}")]
    Geo(#[from] medallion::GeoError),
    #[error("partitioning the dataset: {0}")]
    Path(#[from] medallion::PathError),
    #[error("writing the dataset: {0}")]
    Write(#[from] medallion::WriteError),
}

/// One deduped leg as the query returns it: the derived attributes plus the still-encoded
/// polyline the geometry columns are built from.
#[derive(Debug, Deserialize)]
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

/// The attributes of one derived leg. The geometry columns are appended separately, since
/// they carry GeoArrow metadata a serde type cannot express.
#[derive(Debug, Serialize, Deserialize)]
struct TrainSegmentRow {
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
}

impl From<&Leg> for TrainSegmentRow {
    fn from(leg: &Leg) -> Self {
        Self {
            trip_id: leg.trip_id.clone(),
            route_name: leg.route_name.clone(),
            train_number: leg.train_number,
            agency_id: leg.agency_id.clone(),
            agency_name: leg.agency_name.clone(),
            mode: leg.mode.clone(),
            route_color: leg.route_color.clone(),
            realtime: leg.realtime,
            from_stop_id: leg.from_stop_id.clone(),
            departure: leg.departure,
            arrival: leg.arrival,
        }
    }
}

/// Derive the silver dataset from the bronze capture log in the same store.
///
/// Dedup and partitioning are one SQL query each against the capture log; the polyline
/// decoding and projection either query does not express happen per partition in Rust.
pub async fn ingest(root: &Root) -> Result<IngestOutcome, IngestError> {
    let query = Query::new(root.clone());
    if !query
        .register_if_present(Layer::Bronze, crate::bronze::DATASET, CAPTURED)
        .await?
    {
        return Ok(IngestOutcome::default());
    }

    let read: usize = query
        .rows::<Counted>("SELECT COUNT(*) AS count FROM captured")
        .await?
        .first()
        .map_or(0, |counted| counted.count as usize);

    let dates: Vec<DepartureDate> = query
        .rows(&format!(
            "SELECT DISTINCT CAST(departure AS DATE) AS date FROM ({DEDUPED}) ORDER BY date"
        ))
        .await?;

    let projector = Projector::new()?;
    let mut deduped = 0;
    for DepartureDate { date } in &dates {
        let legs: Vec<Leg> = query
            .rows(&format!(
                "SELECT trip_id, route_name, train_number, agency_id, agency_name, mode,
                        route_color, realtime, from_stop_id, departure, arrival, polyline
                 FROM ({DEDUPED})
                 WHERE CAST(departure AS DATE) = DATE '{date}'"
            ))
            .await?;
        deduped += legs.len();

        root.dataset(Layer::Silver, DATASET)
            .date_partition("departure_date", *date)?
            .rebuild_geo(&[batch(&legs, &projector)?])
            .await?;
    }

    Ok(IngestOutcome {
        read,
        deduped,
        partitions: dates.len(),
    })
}

/// A `COUNT(*)` result.
#[derive(Debug, Deserialize)]
struct Counted {
    count: i64,
}

/// One date the derived dataset is partitioned by.
#[derive(Debug, Deserialize)]
struct DepartureDate {
    date: NaiveDate,
}

/// Build one partition's batch: the attribute columns, then the two geometry columns.
fn batch(legs: &[Leg], projector: &Projector) -> Result<RecordBatch, IngestError> {
    let attributes: Vec<TrainSegmentRow> = legs.iter().map(TrainSegmentRow::from).collect();
    let mut fields = attribute_fields()?;
    let mut arrays = serde_arrow::to_arrow(&fields, &attributes)?;

    let lines = legs
        .iter()
        .map(|leg| decode_polyline(&leg.polyline))
        .collect::<Result<Vec<_>, _>>()?;
    let projected = lines
        .iter()
        .map(|line| projector.project(line))
        .collect::<Result<Vec<_>, _>>()?;

    fields.push(medallion::wkb_field(GEOMETRY)?.into());
    arrays.push(wkb_column(&lines)?);
    fields.push(medallion::projected_wkb_field(PROJECTED_GEOMETRY)?.into());
    arrays.push(wkb_column(&projected)?);

    Ok(RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)?)
}

/// The arrow schema of the attribute columns, with the instant columns typed.
fn attribute_fields() -> Result<Vec<FieldRef>, IngestError> {
    let options =
        INSTANT_COLUMNS
            .iter()
            .try_fold(TracingOptions::default(), |options, &name| {
                options.overwrite(
                    name,
                    json!({"name": name, "data_type": "Timestamp(Millisecond, Some(\"UTC\"))"}),
                )
            })?;
    Ok(Vec::<FieldRef>::from_type::<TrainSegmentRow>(options)?)
}

/// Encode each line as `(lon, lat)` WKB — little-endian, OGC.
fn wkb_column(lines: &[LineString<f64>]) -> Result<ArrayRef, IngestError> {
    let encoded = lines
        .iter()
        .map(|line| {
            let mut buf = Vec::new();
            write_line_string(&mut buf, line, &WriteOptions::default())?;
            Ok(buf)
        })
        .collect::<Result<Vec<_>, wkb::error::WkbError>>()?;
    Ok(Arc::new(BinaryArray::from_iter_values(encoded)))
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
    use geo_traits::{CoordTrait, GeometryTrait, GeometryType, LineStringTrait};
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

    /// The derived dataset, read back the way any other reader would: as a table.
    async fn derived(root: &Root, sql: &str) -> Vec<RecordBatch> {
        let query = Query::new(root.clone());
        query
            .register(Layer::Silver, DATASET, "derived")
            .await
            .expect("register derived dataset");
        query.sql(sql).await.expect("query derived dataset")
    }

    /// The single `count` a `COUNT(*)` query returns.
    fn count(batches: &[RecordBatch]) -> i64 {
        batches[0]
            .column_by_name("count")
            .expect("count column")
            .as_any()
            .downcast_ref::<arrow::array::Int64Array>()
            .expect("i64 count")
            .value(0)
    }

    /// The line held in `column` of `batch`'s first row.
    fn first_line(batch: &RecordBatch, column: &str) -> Vec<(f64, f64)> {
        let geometries = arrow::compute::cast(
            batch.column_by_name(column).expect(column),
            &arrow::datatypes::DataType::Binary,
        )
        .expect("cast to binary");
        let geometries = geometries
            .as_any()
            .downcast_ref::<BinaryArray>()
            .expect("binary column");
        let geometry = wkb::reader::read_wkb(geometries.value(0)).expect("valid WKB");
        let GeometryType::LineString(line) = geometry.as_type() else {
            panic!("{column} should hold a LineString");
        };
        line.coords().map(|c| (c.x(), c.y())).collect()
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

        let kept = derived(
            &root,
            &format!(
                "SELECT COUNT(*) AS count FROM derived WHERE realtime = {}",
                later[0].real_time
            ),
        )
        .await;
        assert_eq!(
            count(&kept),
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

        let batches = derived(
            &root,
            &format!(
                "SELECT ST_AsBinary({GEOMETRY}) AS {GEOMETRY},
                        ST_AsBinary({PROJECTED_GEOMETRY}) AS {PROJECTED_GEOMETRY}
                 FROM derived ORDER BY trip_id"
            ),
        )
        .await;
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

        let batches = derived(
            &root,
            &format!(
                "SELECT ST_AsBinary({GEOMETRY}) AS {GEOMETRY},
                        ST_AsBinary({PROJECTED_GEOMETRY}) AS {PROJECTED_GEOMETRY}
                 FROM derived ORDER BY trip_id"
            ),
        )
        .await;
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
        let rows_before = count(&derived(&root, "SELECT COUNT(*) AS count FROM derived").await);
        let second = ingest(&root).await.expect("re-ingest");

        assert_eq!(first, second, "the same run, run twice");
        assert_eq!(
            count(&derived(&root, "SELECT COUNT(*) AS count FROM derived").await),
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
