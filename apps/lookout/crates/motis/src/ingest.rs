use crate::legacy::legacy_source_id_sql;
use crate::segment::{NotAPolyline, decode_polyline};
use chrono::{DateTime, Utc};
use domain::TrainNumber;
use geo_types::{Coord, LineString, Point};
use medallion::{Countries, DatasetSpec, GeoRow, Query, Root, layers};
use medallion_model::{
    AgencyId, MOTIS_SEGMENT, MOTIS_SEGMENT_V2, MotisSourceId, RailMode, StopId, TrainSegmentRowV3,
    TripId,
};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;

const LEG_IDENTITY: &str = "trip_id, from_stop_id, scheduled_departure";

const LEG_COLUMNS: &str = "trip_id, route_name, train_number, agency_id, agency_name, mode,
    realtime, from_stop_id, from_lat, from_lon, to_lat, to_lon, departure, arrival, polyline";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IngestOutcome {
    pub read: usize,
    pub deduped: usize,
    pub partitions: usize,
    pub not_rail: usize,
    pub unplaceable: usize,
    pub shapeless: usize,
    pub shape_corrected: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("querying the capture log: {0}")]
    Query(#[from] medallion::QueryError),
    #[error(transparent)]
    Polyline(#[from] NotAPolyline),
    #[error("writing the dataset: {0}")]
    Write(#[from] medallion::TableError),
    #[error("a capture names the source `{source_id}`, which was never registered")]
    UnregisteredSource { source_id: MotisSourceId },
    #[error("reading the registered sources: {0}")]
    Sources(#[from] crate::source::SourceError),
    #[error("placing a train: {0}")]
    Unplaced(#[from] crate::sampling::NoWhereabouts),
    #[error("projecting a train's sample: {0}")]
    Projection(#[from] medallion::GeoError),
    #[error("trip `{trip_id}`: {source}")]
    InvalidLeg {
        trip_id: TripId,
        #[source]
        source: crate::sampling::InvalidLeg,
    },
    #[error("trip `{trip_id}`: {source}")]
    InvalidLegs {
        trip_id: TripId,
        #[source]
        source: crate::sampling::InvalidLegs,
    },
    #[error("trip `{trip_id}` sampled to no samples")]
    NoSamples { trip_id: TripId },
    #[error("trip `{trip_id}` has more samples than a u32 can number")]
    TooManySamples { trip_id: TripId },
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Leg {
    pub(crate) source_id: MotisSourceId,
    pub(crate) trip_id: TripId,
    pub(crate) route_name: Option<String>,
    pub(crate) train_number: Option<TrainNumber>,
    pub(crate) agency_id: Option<AgencyId>,
    pub(crate) agency_name: Option<String>,
    pub(crate) mode: RailMode,
    pub(crate) realtime: bool,
    pub(crate) from_stop_id: Option<StopId>,
    pub(crate) from_lat: f64,
    pub(crate) from_lon: f64,
    pub(crate) to_lat: f64,
    pub(crate) to_lon: f64,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub(crate) departure: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub(crate) arrival: DateTime<Utc>,
    pub(crate) polyline: String,
}

impl From<&Leg> for TrainSegmentRowV3 {
    fn from(leg: &Leg) -> Self {
        Self {
            source_id: leg.source_id.clone(),
            trip_id: leg.trip_id.clone(),
            route_name: leg.route_name.clone(),
            train_number: leg.train_number,
            agency_id: leg.agency_id.clone(),
            agency_name: leg.agency_name.clone(),
            mode: leg.mode,
            realtime: leg.realtime,
            from_stop_id: leg.from_stop_id.clone(),
            departure: leg.departure,
            arrival: leg.arrival,
        }
    }
}

pub(crate) struct Captured {
    pub(crate) read: usize,
    pub(crate) not_rail: usize,
    pub(crate) legs: Vec<Leg>,
}

pub async fn ingest(root: &Root, countries: &impl Countries) -> Result<IngestOutcome, IngestError> {
    let captures = captures(root).await?;

    let mut outcome = IngestOutcome::default();
    let mut placed: Vec<GeoRow<TrainSegmentRowV3, LineString<f64>>> = Vec::new();
    for captured in captures.into_iter().flatten() {
        outcome.read += captured.read;
        outcome.not_rail += captured.not_rail;
        outcome.deduped += captured.legs.len() + captured.not_rail;
        for leg in &captured.legs {
            let line = match shape_of(leg)? {
                Shape::Captured(line) => Some(line),
                Shape::Corrected(line) => {
                    outcome.shape_corrected += 1;
                    Some(line)
                }
                Shape::Missing => {
                    outcome.shapeless += 1;
                    None
                }
            };
            if let Some(line) = line {
                match countries.containing(starts_from(&line)) {
                    Some(country) => placed.push(GeoRow {
                        row: TrainSegmentRowV3::from(leg),
                        geometry: line,
                        country,
                    }),
                    None => outcome.unplaceable += 1,
                }
            }
        }
    }

    outcome.partitions = medallion::write_geo_rows(root, &placed)
        .await?
        .partitions
        .written;
    Ok(outcome)
}

pub(crate) async fn captures(root: &Root) -> Result<[Option<Captured>; 2], IngestError> {
    let query = Query::new(root.clone());
    let legacy_source_id = legacy_source_id_sql("captured_at");
    Ok([
        captured(
            &query,
            MOTIS_SEGMENT,
            &format!("{legacy_source_id} AS source_id"),
            &format!("{legacy_source_id}, {LEG_IDENTITY}"),
        )
        .await?,
        captured(
            &query,
            MOTIS_SEGMENT_V2,
            "source_id",
            &format!("source_id, {LEG_IDENTITY}"),
        )
        .await?,
    ])
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
    let rail_modes = rail_modes();
    let not_rail = query
        .count(&format!(
            "SELECT COUNT(*) AS count FROM (
               SELECT DISTINCT {one_leg} FROM {table} WHERE mode NOT IN ({rail_modes})
             )"
        ))
        .await? as usize;
    let legs = query
        .rows(&format!(
            "SELECT {source}, {LEG_COLUMNS}
             FROM (
               SELECT *, ROW_NUMBER() OVER (
                 PARTITION BY {one_leg} ORDER BY captured_at DESC
               ) AS rank
               FROM {table}
               WHERE mode IN ({rail_modes})
             )
             WHERE rank = 1
             ORDER BY departure"
        ))
        .await?;
    Ok(Some(Captured {
        read,
        not_rail,
        legs,
    }))
}

fn rail_modes() -> String {
    RailMode::iter()
        .map(|mode| format!("'{}'", mode.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) enum Shape {
    Captured(LineString<f64>),
    Corrected(LineString<f64>),
    Missing,
}

pub(crate) fn shape_of(leg: &Leg) -> Result<Shape, NotAPolyline> {
    let line = decode_polyline(&leg.polyline)?;
    if has_two_distinct_points(&line) {
        return Ok(Shape::Captured(line));
    }
    let from = Coord {
        x: leg.from_lon,
        y: leg.from_lat,
    };
    let to = Coord {
        x: leg.to_lon,
        y: leg.to_lat,
    };
    Ok(match from != to {
        true => Shape::Corrected(LineString::new(vec![from, to])),
        false => Shape::Missing,
    })
}

fn has_two_distinct_points(line: &LineString<f64>) -> bool {
    line.0
        .first()
        .is_some_and(|first| line.0.iter().any(|coord| coord != first))
}

fn starts_from(line: &LineString<f64>) -> Point<f64> {
    line.points()
        .next()
        .expect("a polyline decodes to at least one point")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap as Map;

    use crate::bronze::{SegmentLog, segment_row};
    use crate::segment::SingleTripSegment;

    use crate::api::types::{Mode, TripSegment};
    use arrow::array::RecordBatch;
    use chrono::TimeZone;
    use medallion::{GEOMETRY, PROJECTED_GEOMETRY};
    use medallion_model::{Feed, MotisSegmentRow};

    use super::*;
    use crate::legacy::legacy_sources;
    use crate::testing::{Nowhere, germany, regional_trips, source};

    fn log(root: &Root) -> SegmentLog {
        SegmentLog::new(root.clone(), source(Feed::Local).id())
    }

    async fn append_legacy(root: &Root, captured_at: DateTime<Utc>) {
        let rows: Vec<MotisSegmentRow> = one_trip_each(regional_trips())
            .iter()
            .map(|segment| segment_row(captured_at, segment, &Map::new()))
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

    fn one_trip(segment: TripSegment) -> SingleTripSegment {
        SingleTripSegment::new(segment).expect("a segment with one trip")
    }

    fn one_trip_each(segments: Vec<TripSegment>) -> Vec<SingleTripSegment> {
        segments.into_iter().map(one_trip).collect()
    }

    async fn ingest_polls(root: &Root, polls: &[DateTime<Utc>]) -> IngestOutcome {
        let log = log(root);
        for captured_at in polls {
            log.append(*captured_at, &one_trip_each(regional_trips()), &Map::new())
                .await
                .expect("append poll");
        }
        ingest(root, &germany()).await.expect("ingest")
    }

    async fn derived(root: &Root) -> Query {
        derived_from(root, medallion_model::TRAIN_SEGMENT_V3).await
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

        assert_eq!(outcome.read, regional_trips().len() * polls.len());
        assert_eq!(outcome.deduped, regional_trips().len());
    }

    #[tokio::test]
    async fn the_newest_capture_of_a_leg_wins() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let log = log(&root);
        let mut later = regional_trips();
        for segment in &mut later {
            segment.real_time = !segment.real_time;
        }

        log.append(
            Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap(),
            &one_trip_each(regional_trips()),
            &Map::new(),
        )
        .await
        .expect("first poll");
        log.append(
            Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 30).unwrap(),
            &one_trip_each(later.clone()),
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
            regional_trips().len() as i64,
            "every leg should carry the later capture's realtime flag"
        );
    }

    #[tokio::test]
    async fn a_delayed_leg_is_kept_once() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let log = log(&root);
        let delay = chrono::Duration::minutes(3);
        let mut delayed = regional_trips();
        for segment in &mut delayed {
            segment.departure += delay;
            segment.arrival += delay;
        }

        log.append(
            Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap(),
            &one_trip_each(regional_trips()),
            &Map::new(),
        )
        .await
        .expect("first poll");
        log.append(
            Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 30).unwrap(),
            &one_trip_each(delayed),
            &Map::new(),
        )
        .await
        .expect("second poll");
        let outcome = ingest(&root, &germany()).await.expect("ingest");

        assert_eq!(outcome.deduped, regional_trips().len());
    }

    #[tokio::test]
    async fn each_leg_keeps_its_polyline_as_a_line_string() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let expected = polyline::decode_polyline(
            &regional_trips()[0].polyline,
            crate::segment::POLYLINE_PRECISION,
        )
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

    fn shaped(points: &[(f64, f64)]) -> TripSegment {
        let mut segment = regional_trips().remove(0);
        segment.polyline = polyline::encode_coordinates(
            points.iter().map(|&(x, y)| geo_types::Coord { x, y }),
            crate::segment::POLYLINE_PRECISION,
        )
        .expect("encode");
        segment
    }

    fn between_stops(segment: &TripSegment) -> Vec<(f64, f64)> {
        vec![
            (segment.from.lon, segment.from.lat),
            (segment.to.lon, segment.to.lat),
        ]
    }

    async fn ingest_one(root: &Root, segment: &TripSegment) -> IngestOutcome {
        log(root)
            .append(
                Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap(),
                &[one_trip(segment.clone())],
                &Map::new(),
            )
            .await
            .expect("append poll");
        ingest(root, &germany()).await.expect("ingest")
    }

    async fn line_of_one(segment: &TripSegment) -> (Vec<(f64, f64)>, usize) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let outcome = ingest_one(&root, segment).await;
        (
            first_line(&geometry_batches(&root).await[0], GEOMETRY),
            outcome.shape_corrected,
        )
    }

    fn close(actual: &[(f64, f64)], expected: &[(f64, f64)]) -> bool {
        actual.len() == expected.len()
            && actual
                .iter()
                .zip(expected)
                .all(|(a, e)| (a.0 - e.0).abs() < 1e-5 && (a.1 - e.1).abs() < 1e-5)
    }

    #[tokio::test]
    async fn a_leg_whose_shape_is_one_point_runs_straight_between_its_stops() {
        let start = regional_trips()[0].from.clone();
        let segment = shaped(&[(start.lon, start.lat)]);

        let (line, corrected) = line_of_one(&segment).await;

        assert!(close(&line, &between_stops(&segment)), "{line:?}");
        assert_eq!(corrected, 1);
    }

    #[tokio::test]
    async fn a_leg_whose_shape_is_empty_runs_straight_between_its_stops() {
        let segment = shaped(&[]);

        let (line, corrected) = line_of_one(&segment).await;

        assert!(close(&line, &between_stops(&segment)), "{line:?}");
        assert_eq!(corrected, 1);
    }

    #[tokio::test]
    async fn a_leg_whose_shape_repeats_one_point_runs_straight_between_its_stops() {
        let start = regional_trips()[0].from.clone();
        let segment = shaped(&[(start.lon, start.lat), (start.lon, start.lat)]);

        let (line, corrected) = line_of_one(&segment).await;

        assert!(close(&line, &between_stops(&segment)), "{line:?}");
        assert_eq!(corrected, 1);
    }

    #[tokio::test]
    async fn a_leg_with_a_shape_keeps_it_uncorrected() {
        let segment = regional_trips().remove(0);
        let captured: Vec<(f64, f64)> =
            polyline::decode_polyline(&segment.polyline, crate::segment::POLYLINE_PRECISION)
                .expect("decode")
                .coords()
                .map(|c| (c.x, c.y))
                .collect();

        let (line, corrected) = line_of_one(&segment).await;

        assert!(close(&line, &captured), "{line:?}");
        assert_eq!(corrected, 0);
    }

    #[tokio::test]
    async fn a_shapeless_leg_between_stops_in_one_place_is_left_out_and_counted() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let mut segment = shaped(&[]);
        segment.to.lat = segment.from.lat;
        segment.to.lon = segment.from.lon;

        let outcome = ingest_one(&root, &segment).await;

        assert_eq!((outcome.deduped, outcome.shapeless), (1, 1));
        assert_eq!(outcome.partitions, 0);
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

        let partitions: Vec<_> = std::fs::read_dir(root.path().join("silver/train_segment_v3"))
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
    async fn a_leg_of_a_mode_the_capture_drops_is_left_out_and_counted() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let mut segments = regional_trips();
        segments[0].mode = Mode::Ferry;
        log(&root)
            .append(
                Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap(),
                &one_trip_each(segments.clone()),
                &Map::new(),
            )
            .await
            .expect("append poll");

        let outcome = ingest(&root, &germany()).await.expect("ingest");

        assert_eq!((outcome.deduped, outcome.not_rail), (segments.len(), 1));
        let ferries = derived(&root)
            .await
            .count(&format!(
                "SELECT COUNT(*) AS count FROM derived WHERE trip_id = '{}'",
                segments[0].trip_id().expect("a segment with one trip")
            ))
            .await
            .expect("count");
        assert_eq!(ferries, 0);
    }

    async fn refusal_of(blank: impl FnOnce(&mut MotisSegmentRow)) -> String {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let captured_at = Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap();
        let mut row = segment_row(
            captured_at,
            &one_trip(regional_trips().remove(0)),
            &Map::new(),
        );
        blank(&mut row);
        root.rows_of::<MotisSegmentRow>()
            .on_date(captured_at.date_naive())
            .expect("partition")
            .append_rows(captured_at, &[row])
            .await
            .expect("append legacy poll");

        ingest(&root, &germany())
            .await
            .expect_err("an empty id")
            .to_string()
    }

    #[tokio::test]
    async fn an_empty_trip_id_in_the_capture_log_is_refused() {
        let refusal = refusal_of(|row| row.trip_id = String::new()).await;

        assert!(refusal.contains("a trip id cannot be empty"), "{refusal}");
    }

    #[tokio::test]
    async fn an_empty_stop_id_in_the_capture_log_is_refused() {
        let refusal = refusal_of(|row| row.from_stop_id = Some(String::new())).await;

        assert!(refusal.contains("a stop id cannot be empty"), "{refusal}");
    }

    #[tokio::test]
    async fn a_leg_outside_every_known_country_is_not_written() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let log = log(&root);
        log.append(
            Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap(),
            &one_trip_each(regional_trips()),
            &Map::new(),
        )
        .await
        .expect("append poll");

        let outcome = ingest(&root, &Nowhere).await.expect("ingest");

        assert_eq!(outcome.unplaceable, regional_trips().len());
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
    async fn a_legacy_capture_before_the_move_to_delfi_is_derived_under_gtfs_de() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        append_legacy(
            &root,
            Utc.with_ymd_and_hms(2026, 7, 22, 23, 59, 59).unwrap(),
        )
        .await;

        ingest(&root, &germany()).await.expect("ingest");

        let [gtfs_de, _] = legacy_sources().map(|source| source.id().to_string());
        assert_eq!(sources_derived(&root).await, [gtfs_de]);
    }

    #[tokio::test]
    async fn a_legacy_capture_from_the_move_to_delfi_is_derived_under_delfi() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        append_legacy(&root, Utc.with_ymd_and_hms(2026, 7, 23, 0, 0, 0).unwrap()).await;

        ingest(&root, &germany()).await.expect("ingest");

        let [_, delfi] = legacy_sources().map(|source| source.id().to_string());
        assert_eq!(sources_derived(&root).await, [delfi]);
    }

    #[tokio::test]
    async fn the_same_leg_from_two_sources_is_kept_once_for_each() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let at = Utc.with_ymd_and_hms(2026, 7, 26, 14, 0, 0).unwrap();
        log(&root)
            .append(at, &one_trip_each(regional_trips()), &Map::new())
            .await
            .expect("append poll");
        SegmentLog::new(root.clone(), source(Feed::Transitous).id())
            .append(
                at + chrono::Duration::seconds(1),
                &one_trip_each(regional_trips()),
                &Map::new(),
            )
            .await
            .expect("append poll");

        let outcome = ingest(&root, &germany()).await.expect("ingest");

        assert_eq!(outcome.read, regional_trips().len() * 2);
        assert_eq!(outcome.deduped, regional_trips().len() * 2);
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
