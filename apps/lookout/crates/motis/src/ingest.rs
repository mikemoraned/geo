use std::ops::Add;

use chrono::{DateTime, Utc};
use domain::TrainNumber;
use geo_types::{LineString, Point};
use medallion::{Countries, DatasetSpec, Dated, GeoRow, Query, Root, layers};
use medallion_model::{
    MOTIS_SEGMENT, MOTIS_SEGMENT_V2, MotisSourceId, TrainSegmentRowV1, TrainSegmentRowV2,
};
use serde::{Deserialize, Serialize};

const POLYLINE_PRECISION: u32 = 5;

const LEG_COLUMNS: &str = "trip_id, route_name, train_number, agency_id, agency_name, mode,
    route_color, realtime, from_stop_id, departure, arrival, polyline";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IngestOutcome {
    pub read: usize,
    pub deduped: usize,
    pub partitions: usize,
    pub unplaceable: usize,
}

impl Add for IngestOutcome {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            read: self.read + other.read,
            deduped: self.deduped + other.deduped,
            partitions: self.partitions + other.partitions,
            unplaceable: self.unplaceable + other.unplaceable,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("querying the capture log: {0}")]
    Query(#[from] medallion::QueryError),
    #[error("decoding polyline: {0}")]
    Polyline(String),
    #[error("writing the dataset: {0}")]
    Write(#[from] medallion::TableError),
    #[error("a capture of trip `{trip_id}` names no source")]
    Unsourced { trip_id: String },
}

#[derive(Debug, Serialize, Deserialize)]
struct Leg {
    source_id: Option<MotisSourceId>,
    trip_id: String,
    route_name: Option<String>,
    train_number: Option<TrainNumber>,
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

impl From<&Leg> for TrainSegmentRowV1 {
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

impl TryFrom<&Leg> for TrainSegmentRowV2 {
    type Error = IngestError;

    fn try_from(leg: &Leg) -> Result<Self, Self::Error> {
        let source_id = leg
            .source_id
            .clone()
            .ok_or_else(|| IngestError::Unsourced {
                trip_id: leg.trip_id.clone(),
            })?;
        Ok(Self {
            source_id,
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
        })
    }
}

struct Captured {
    read: usize,
    legs: Vec<Leg>,
}

pub async fn ingest(root: &Root, countries: &impl Countries) -> Result<IngestOutcome, IngestError> {
    let query = Query::new(root.clone());
    let mut outcome = IngestOutcome::default();

    if let Some(captured) = captured(
        &query,
        MOTIS_SEGMENT,
        "CAST(NULL AS VARCHAR) AS source_id",
        "trip_id, from_stop_id, departure",
    )
    .await?
    {
        outcome = outcome
            + derive(root, countries, captured, |leg| {
                Ok(TrainSegmentRowV1::from(leg))
            })
            .await?;
    }

    if let Some(captured) = captured(
        &query,
        MOTIS_SEGMENT_V2,
        "source_id",
        "source_id, trip_id, from_stop_id, departure",
    )
    .await?
    {
        outcome = outcome
            + derive(root, countries, captured, |leg| {
                TrainSegmentRowV2::try_from(leg)
            })
            .await?;
    }

    Ok(outcome)
}

async fn captured(
    query: &Query,
    dataset: DatasetSpec<layers::Bronze>,
    source: &str,
    one_leg: &str,
) -> Result<Option<Captured>, IngestError> {
    if !query.register_if_present(dataset, dataset.name).await? {
        return Ok(None);
    }
    let table = dataset.name;
    let read = query
        .count(&format!("SELECT COUNT(*) AS count FROM {table}"))
        .await? as usize;
    let legs = query
        .rows(&format!(
            "SELECT {source}, {LEG_COLUMNS}
             FROM (
               SELECT *, ROW_NUMBER() OVER (
                 PARTITION BY {one_leg} ORDER BY captured_at DESC
               ) AS rank
               FROM {table}
             )
             WHERE rank = 1
             ORDER BY departure"
        ))
        .await?;
    Ok(Some(Captured { read, legs }))
}

async fn derive<R>(
    root: &Root,
    countries: &impl Countries,
    captured: Captured,
    row: impl Fn(&Leg) -> Result<R, IngestError>,
) -> Result<IngestOutcome, IngestError>
where
    R: Dated<Layer = layers::Silver> + Clone,
{
    let mut outcome = IngestOutcome {
        read: captured.read,
        deduped: captured.legs.len(),
        ..IngestOutcome::default()
    };
    let mut placed: Vec<GeoRow<R, LineString<f64>>> = Vec::new();
    for leg in &captured.legs {
        let line = decode_polyline(&leg.polyline)?;
        match countries.containing(starts_from(&line)) {
            Some(country) => placed.push(GeoRow {
                row: row(leg)?,
                geometry: line,
                country,
            }),
            None => outcome.unplaceable += 1,
        }
    }

    outcome.partitions = medallion::write_geo_rows(root, &placed)
        .await?
        .partitions
        .written;
    Ok(outcome)
}

fn starts_from(line: &LineString<f64>) -> Point<f64> {
    line.points()
        .next()
        .expect("a polyline decodes to at least one point")
}

fn decode_polyline(encoded: &str) -> Result<LineString<f64>, IngestError> {
    polyline::decode_polyline(encoded, POLYLINE_PRECISION)
        .map_err(|e| IngestError::Polyline(e.to_string()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap as Map;

    use crate::bronze::{SegmentLog, segment_row};

    use crate::api::types::TripSegment;
    use arrow::array::RecordBatch;
    use chrono::TimeZone;
    use medallion::{Country, GEOMETRY, PROJECTED_GEOMETRY};
    use medallion_model::{Feed, MotisSegmentRow, MotisSource};
    use url::Url;

    use super::*;

    struct Everywhere(Country);

    impl Countries for Everywhere {
        fn containing(&self, _point: Point<f64>) -> Option<Country> {
            Some(self.0)
        }
    }

    struct Nowhere;

    impl Countries for Nowhere {
        fn containing(&self, _point: Point<f64>) -> Option<Country> {
            None
        }
    }

    fn germany() -> Everywhere {
        Everywhere(Country::Germany)
    }

    fn source(feed: Feed) -> MotisSource {
        MotisSource {
            base_url: Url::parse("http://127.0.0.1:8080").expect("a URL"),
            motis_version: "v2.11.3".parse().expect("a version"),
            feed,
            area: None,
        }
    }

    fn log(root: &Root) -> SegmentLog {
        SegmentLog::new(root.clone(), source(Feed::Local).id())
    }

    async fn append_legacy(root: &Root, captured_at: DateTime<Utc>) {
        let rows: Vec<MotisSegmentRow> = fixture()
            .iter()
            .map(|segment| {
                segment_row(captured_at, segment, &Map::new()).expect("a segment with one trip")
            })
            .collect();
        root.rows_of::<MotisSegmentRow>()
            .on_date(captured_at.date_naive())
            .expect("partition")
            .append_rows(captured_at, &rows)
            .await
            .expect("append legacy poll");
    }

    async fn sources_derived(root: &Root) -> Vec<String> {
        #[derive(Deserialize)]
        struct Named {
            source_id: String,
        }
        derived(root)
            .await
            .rows::<Named>("SELECT DISTINCT source_id FROM derived ORDER BY source_id")
            .await
            .expect("sources")
            .into_iter()
            .map(|named| named.source_id)
            .collect()
    }

    fn fixture() -> Vec<TripSegment> {
        serde_json::from_str(include_str!("../tests/fixtures/trips.json")).expect("parse fixture")
    }

    async fn ingest_polls(root: &Root, polls: &[DateTime<Utc>]) -> IngestOutcome {
        let log = log(root);
        for captured_at in polls {
            log.append(*captured_at, &fixture(), &Map::new())
                .await
                .expect("append poll");
        }
        ingest(root, &germany()).await.expect("ingest")
    }

    async fn derived(root: &Root) -> Query {
        derived_from(root, medallion_model::TRAIN_SEGMENT_V2).await
    }

    async fn derived_from(root: &Root, dataset: DatasetSpec<layers::Silver>) -> Query {
        let query = Query::new(root.clone());
        query
            .register(dataset, "derived")
            .await
            .expect("register derived dataset");
        query
    }

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
        let log = log(&root);
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
        ingest(&root, &germany()).await.expect("ingest");

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
        let second = ingest(&root, &germany()).await.expect("re-ingest");

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
    async fn a_leg_is_written_under_the_country_it_starts_in() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());

        ingest_polls(
            &root,
            &[Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap()],
        )
        .await;

        let partitions: Vec<_> = std::fs::read_dir(root.path().join("silver/train_segment_v2"))
            .expect("dataset dir")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(partitions, ["country=DE"]);
    }

    #[tokio::test]
    async fn a_leg_outside_every_known_country_is_not_written() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let log = log(&root);
        log.append(
            Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap(),
            &fixture(),
            &Map::new(),
        )
        .await
        .expect("append poll");

        let outcome = ingest(&root, &Nowhere).await.expect("ingest");

        assert_eq!(outcome.unplaceable, fixture().len());
        assert_eq!(outcome.partitions, 0);
        assert!(!root.path().join("silver").exists());
    }

    #[tokio::test]
    async fn a_leg_names_the_source_that_captured_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());

        ingest_polls(
            &root,
            &[Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap()],
        )
        .await;

        assert_eq!(
            sources_derived(&root).await,
            [source(Feed::Local).id().to_string()]
        );
    }

    #[tokio::test]
    async fn a_capture_from_before_sources_were_recorded_is_derived_into_the_first_version() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        append_legacy(&root, Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap()).await;

        let outcome = ingest(&root, &germany()).await.expect("ingest");

        assert_eq!(outcome.deduped, fixture().len());
        assert_eq!(
            derived_from(&root, medallion_model::TRAIN_SEGMENT)
                .await
                .count("SELECT COUNT(*) AS count FROM derived")
                .await
                .expect("count"),
            fixture().len() as i64
        );
        assert!(!root.path().join("silver/train_segment_v2").exists());
    }

    #[tokio::test]
    async fn the_same_leg_from_two_sources_is_kept_once_for_each() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let at = Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap();
        log(&root)
            .append(at, &fixture(), &Map::new())
            .await
            .expect("append poll");
        SegmentLog::new(root.clone(), source(Feed::Transitous).id())
            .append(at + chrono::Duration::seconds(1), &fixture(), &Map::new())
            .await
            .expect("append poll");

        let outcome = ingest(&root, &germany()).await.expect("ingest");

        assert_eq!(outcome.read, fixture().len() * 2);
        assert_eq!(outcome.deduped, fixture().len() * 2);
    }

    #[tokio::test]
    async fn an_empty_capture_log_derives_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let outcome = ingest(&Root::new(tmp.path()), &germany())
            .await
            .expect("ingest");

        assert_eq!(outcome, IngestOutcome::default());
    }
}
