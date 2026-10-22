use std::collections::HashMap;
use std::collections::hash_map::Entry;

use chrono::TimeDelta;
use domain::{Bbox, Span};
use geo::{BoundingRect, Distance, Euclidean};
use geo_types::{LineString, Point};
use medallion::{Countries, Country, GeoRow, GeoRowWriter, Projector, Replaced, Root};
use medallion_model::{
    MotisSourceId, TrainSessionRow, TrainSessionSampleRow, TripId, train_session_id,
};

use crate::ingest::{self, IngestError, Shape, captures, shape_of};
use crate::sampling::{InvalidLeg, InvalidLegs, Leg, NormalisedLegs, Placed};

const SAMPLES_PER_WRITE: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrainSessionsOutcome {
    pub trips: usize,
    pub sessions: usize,
    pub samples: usize,
    pub dropped: DroppedTrips,
    pub session_partitions: Replaced,
    pub sample_partitions: Replaced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DroppedTrips {
    pub shapeless: usize,
    pub arrives_before_departing: usize,
    pub overlapping: usize,
    pub unplaceable: usize,
}

impl DroppedTrips {
    pub fn total(&self) -> usize {
        self.shapeless + self.arrives_before_departing + self.overlapping + self.unplaceable
    }
}

struct Trip {
    source_id: MotisSourceId,
    source_label: Option<String>,
    trip_id: TripId,
    legs: NormalisedLegs,
}

enum Normalised {
    Trip(Trip),
    Dropped(Drop),
}

enum Drop {
    Shapeless,
    ArrivesBeforeDeparting,
    Overlapping,
}

struct Session {
    row: TrainSessionRow,
    path: LineString<f64>,
    samples: Vec<(TrainSessionSampleRow, Point<f64>)>,
}

pub async fn derive_train_sessions(
    root: &Root,
    countries: &impl Countries,
    interval: Span,
) -> Result<TrainSessionsOutcome, IngestError> {
    let labels: HashMap<MotisSourceId, Option<String>> = crate::source::recorded(root)
        .await?
        .into_iter()
        .map(|source| (source.source_id, source.label))
        .collect();
    let mut by_trip: HashMap<(MotisSourceId, TripId), Vec<ingest::Leg>> = HashMap::new();
    for captured in captures(root, "trip_id, from_stop_id")
        .await?
        .into_iter()
        .flatten()
    {
        for leg in captured.legs {
            let source_id = leg
                .source_id
                .clone()
                .ok_or_else(|| IngestError::Unsourced {
                    trip_id: leg.trip_id.clone(),
                })?;
            by_trip
                .entry((source_id, leg.trip_id.clone()))
                .or_default()
                .push(leg);
        }
    }

    let mut outcome = TrainSessionsOutcome {
        trips: by_trip.len(),
        ..TrainSessionsOutcome::default()
    };
    let mut trips = Vec::with_capacity(by_trip.len());
    for ((source_id, trip_id), legs) in by_trip {
        let source_label =
            labels
                .get(&source_id)
                .cloned()
                .ok_or_else(|| IngestError::UnregisteredSource {
                    source_id: source_id.clone(),
                })?;
        match normalised(source_id, source_label, trip_id, &legs)? {
            Normalised::Trip(trip) => trips.push(trip),
            Normalised::Dropped(Drop::Shapeless) => outcome.dropped.shapeless += 1,
            Normalised::Dropped(Drop::ArrivesBeforeDeparting) => {
                outcome.dropped.arrives_before_departing += 1
            }
            Normalised::Dropped(Drop::Overlapping) => outcome.dropped.overlapping += 1,
        }
    }
    trips.sort_by_key(|trip| trip.legs.departure());

    let mut projectors: HashMap<Country, Projector> = HashMap::new();
    let mut sample_writer = GeoRowWriter::<TrainSessionSampleRow>::new(root)?;
    let mut pending: Vec<GeoRow<TrainSessionSampleRow, Point<f64>>> = Vec::new();
    let mut sessions: Vec<GeoRow<TrainSessionRow, LineString<f64>>> = Vec::new();
    for trip in &trips {
        let placed = trip.legs.sampled(interval)?;
        let start = placed.first().ok_or_else(|| IngestError::NoSamples {
            trip_id: trip.trip_id.clone(),
        })?;
        match countries.containing(start.position) {
            None => outcome.dropped.unplaceable += 1,
            Some(country) => {
                let projector = match projectors.entry(country) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => entry.insert(Projector::for_country(country)?),
                };
                let session = session_of(trip, &placed, projector)?;
                pending.extend(session.samples.into_iter().map(|(row, geometry)| GeoRow {
                    row,
                    geometry,
                    country,
                }));
                sessions.push(GeoRow {
                    row: session.row,
                    geometry: session.path,
                    country,
                });
                if pending.len() >= SAMPLES_PER_WRITE {
                    sample_writer.write(&pending).await?;
                    pending.clear();
                }
            }
        }
    }
    sample_writer.write(&pending).await?;

    let samples = sample_writer.finish().await?;
    outcome.samples = samples.rows;
    outcome.sample_partitions = samples.partitions;
    let written = medallion::write_geo_rows(root, &sessions).await?;
    outcome.sessions = written.rows;
    outcome.session_partitions = written.partitions;
    Ok(outcome)
}

fn normalised(
    source_id: MotisSourceId,
    source_label: Option<String>,
    trip_id: TripId,
    captured: &[ingest::Leg],
) -> Result<Normalised, IngestError> {
    let shapes = captured
        .iter()
        .map(shape_of)
        .collect::<Result<Vec<Shape>, _>>()?;
    let lines: Option<Vec<LineString<f64>>> = shapes
        .into_iter()
        .map(|shape| match shape {
            Shape::Captured(line) | Shape::Corrected(line) => Some(line),
            Shape::Missing => None,
        })
        .collect();
    let Some(lines) = lines else {
        return Ok(Normalised::Dropped(Drop::Shapeless));
    };
    let legs = match captured
        .iter()
        .zip(lines)
        .map(|(leg, line)| Leg::new(leg.departure, leg.arrival, line))
        .collect::<Result<Vec<Leg>, InvalidLeg>>()
    {
        Ok(legs) => legs,
        Err(InvalidLeg::ArrivesBeforeDeparting { .. }) => {
            return Ok(Normalised::Dropped(Drop::ArrivesBeforeDeparting));
        }
        Err(source) => {
            return Err(IngestError::InvalidLeg { trip_id, source });
        }
    };
    match NormalisedLegs::new(&legs) {
        Ok(legs) => Ok(Normalised::Trip(Trip {
            source_id,
            source_label,
            trip_id,
            legs,
        })),
        Err(InvalidLegs::Overlapping { .. }) => Ok(Normalised::Dropped(Drop::Overlapping)),
        Err(source) => Err(IngestError::InvalidLegs { trip_id, source }),
    }
}

fn session_of(
    trip: &Trip,
    placed: &[Placed],
    projector: &Projector,
) -> Result<Session, IngestError> {
    let no_samples = || IngestError::NoSamples {
        trip_id: trip.trip_id.clone(),
    };
    let too_many = || IngestError::TooManySamples {
        trip_id: trip.trip_id.clone(),
    };
    let session_id = train_session_id(&trip.source_id, &trip.trip_id);
    let projected = placed
        .iter()
        .map(|sample| projector.project(&sample.position))
        .collect::<Result<Vec<Point<f64>>, _>>()?;
    let speeds =
        std::iter::once(None).chain(placed.windows(2).zip(projected.windows(2)).map(
            |(times, points)| Some(implied_speed(points[0], points[1], times[1].t - times[0].t)),
        ));

    let samples = placed
        .iter()
        .zip(speeds)
        .enumerate()
        .map(|(seq, (sample, implied_speed_mps))| {
            let row = TrainSessionSampleRow {
                session_id: session_id.clone(),
                source_id: trip.source_id.clone(),
                trip_id: trip.trip_id.clone(),
                t: sample.t,
                seq: u32::try_from(seq).map_err(|_| too_many())?,
                lat: sample.position.y(),
                lon: sample.position.x(),
                implied_speed_mps,
                geometry_source: sample.geometry_source,
            };
            Ok((row, sample.position))
        })
        .collect::<Result<Vec<_>, IngestError>>()?;

    let (Some(first), Some(last)) = (placed.first(), placed.last()) else {
        return Err(no_samples());
    };
    let path = path_through(placed);
    let row = TrainSessionRow {
        session_id,
        source_id: trip.source_id.clone(),
        source_label: trip.source_label.clone(),
        trip_id: trip.trip_id.clone(),
        started_at: first.t,
        ended_at: last.t,
        sample_count: u32::try_from(placed.len()).map_err(|_| too_many())?,
        bbox: Bbox::of(path.bounding_rect().ok_or_else(no_samples)?),
    };
    Ok(Session { row, path, samples })
}

fn path_through(placed: &[Placed]) -> LineString<f64> {
    let coords: Vec<_> = placed.iter().map(|sample| sample.position.0).collect();
    match coords.as_slice() {
        [only] => LineString::new(vec![*only, *only]),
        _ => LineString::new(coords),
    }
}

fn implied_speed(from: Point<f64>, to: Point<f64>, took: TimeDelta) -> f64 {
    Euclidean.distance(from, to) / took.as_seconds_f64()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap as Map;

    use chrono::{DateTime, Duration, TimeZone, Utc};
    use geo_types::Point;
    use medallion::{Country, Query};
    use medallion_model::{
        Feed, MotisSource, TRAIN_SESSION, TRAIN_SESSION_SAMPLE, TripId, train_session_id,
    };
    use serde::Deserialize;
    use url::Url;

    use crate::api::types::{Mode, TripSegment};
    use crate::bronze::SegmentLog;

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

    fn source() -> MotisSource {
        MotisSource {
            base_url: Url::parse("http://127.0.0.1:8080").expect("a URL"),
            motis_version: "v2.11.3".parse().expect("a version"),
            feed: Feed::Local,
            area: None,
            label: Some("delfi-2026-10-10.route-shapes".parse().expect("a label")),
        }
    }

    fn every_10s() -> Span {
        "10s".parse().expect("a span")
    }

    fn fixture() -> Vec<TripSegment> {
        let captured: Vec<TripSegment> =
            serde_json::from_str(include_str!("../tests/fixtures/trips.json"))
                .expect("parse fixture");
        captured
            .into_iter()
            .map(|segment| TripSegment {
                mode: Mode::RegionalRail,
                ..segment
            })
            .collect()
    }

    fn polled_at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 19, 9, minute, 0).unwrap()
    }

    async fn poll(root: &Root, captured_at: DateTime<Utc>, segments: &[TripSegment]) {
        crate::source::register(root, &source(), captured_at)
            .await
            .expect("register the source");
        poll_unregistered(root, captured_at, segments).await;
    }

    async fn poll_unregistered(root: &Root, captured_at: DateTime<Utc>, segments: &[TripSegment]) {
        SegmentLog::new(root.clone(), source().id())
            .append(captured_at, segments, &Map::new())
            .await
            .expect("append poll");
    }

    async fn derive(root: &Root, countries: &impl Countries) -> TrainSessionsOutcome {
        derive_train_sessions(root, countries, every_10s())
            .await
            .expect("derive train sessions")
    }

    async fn count(root: &Root, sql: &str) -> i64 {
        let query = Query::new(root.clone());
        query
            .register(TRAIN_SESSION, "sessions")
            .await
            .expect("register sessions");
        query
            .register(TRAIN_SESSION_SAMPLE, "samples")
            .await
            .expect("register samples");
        query.count(sql).await.expect("count")
    }

    #[derive(Debug, Deserialize)]
    struct Named {
        session_id: String,
        started_at: i64,
    }

    async fn sessions_named(root: &Root) -> Vec<Named> {
        let query = Query::new(root.clone());
        query
            .register(TRAIN_SESSION, "sessions")
            .await
            .expect("register sessions");
        query
            .rows("SELECT session_id, started_at FROM sessions")
            .await
            .expect("sessions")
    }

    fn trip_of(segment: &TripSegment) -> TripId {
        TripId::new(segment.trips[0].trip_id.clone()).expect("a trip id")
    }

    fn onward_from(segment: &TripSegment, departure: DateTime<Utc>) -> TripSegment {
        let mut onward = segment.clone();
        onward.from = segment.to.clone();
        onward.to = segment.from.clone();
        onward.departure = departure;
        onward.arrival = departure + Duration::minutes(10);
        onward
    }

    #[tokio::test]
    async fn each_trip_is_one_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        poll(&root, polled_at(0), &fixture()).await;

        let outcome = derive(&root, &germany()).await;

        assert_eq!((outcome.trips, outcome.sessions), (4, 4));
        assert_eq!(
            count(&root, "SELECT COUNT(*) AS count FROM sessions").await,
            4
        );
    }

    #[tokio::test]
    async fn a_session_is_named_by_its_source_and_trip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let segment = fixture().remove(0);
        poll(&root, polled_at(0), std::slice::from_ref(&segment)).await;

        derive(&root, &germany()).await;

        let id = train_session_id(&source().id(), &trip_of(&segment));
        let named: Vec<String> = sessions_named(&root)
            .await
            .into_iter()
            .map(|session| session.session_id)
            .collect();
        assert_eq!(named, [id.to_string()]);
    }

    #[tokio::test]
    async fn a_session_carries_its_sources_label() {
        #[derive(Deserialize)]
        struct Labelled {
            source_label: Option<String>,
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        poll(&root, polled_at(0), &fixture()[..1]).await;

        derive(&root, &germany()).await;

        let query = Query::new(root.clone());
        query
            .register(TRAIN_SESSION, "sessions")
            .await
            .expect("register sessions");
        let labels: Vec<Option<String>> = query
            .rows::<Labelled>("SELECT source_label FROM sessions")
            .await
            .expect("labels")
            .into_iter()
            .map(|labelled| labelled.source_label)
            .collect();
        assert_eq!(labels, [Some("delfi-2026-10-10.route-shapes".to_string())]);
    }

    #[tokio::test]
    async fn a_capture_from_a_source_never_registered_fails_the_ingest() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        poll_unregistered(&root, polled_at(0), &fixture()[..1]).await;

        let err = derive_train_sessions(&root, &germany(), every_10s())
            .await
            .expect_err("an unregistered source");

        assert!(
            matches!(err, IngestError::UnregisteredSource { .. }),
            "{err}"
        );
    }

    #[tokio::test]
    async fn a_sessions_samples_are_numbered_from_zero_in_time_order() {
        #[derive(Deserialize)]
        struct Numbered {
            seq: u32,
            implied_speed_mps: Option<f64>,
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let segment = fixture().remove(0);
        poll(&root, polled_at(0), std::slice::from_ref(&segment)).await;

        let outcome = derive(&root, &germany()).await;

        let query = Query::new(root.clone());
        query
            .register(TRAIN_SESSION_SAMPLE, "samples")
            .await
            .expect("register samples");
        let samples: Vec<Numbered> = query
            .rows("SELECT seq, implied_speed_mps FROM samples ORDER BY t")
            .await
            .expect("samples");
        let seqs: Vec<u32> = samples.iter().map(|sample| sample.seq).collect();
        assert_eq!(seqs, (0..outcome.samples as u32).collect::<Vec<_>>());
        assert_eq!(samples[0].implied_speed_mps, None);
        assert!(
            samples[1..]
                .iter()
                .all(|sample| sample.implied_speed_mps.is_some())
        );
    }

    #[tokio::test]
    async fn a_leg_recaptured_with_a_later_departure_is_kept_once() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let segment = fixture().remove(0);
        let mut delayed = segment.clone();
        delayed.departure += Duration::minutes(1);
        delayed.arrival += Duration::minutes(1);
        poll(&root, polled_at(0), std::slice::from_ref(&segment)).await;
        poll(&root, polled_at(5), std::slice::from_ref(&delayed)).await;

        let outcome = derive(&root, &germany()).await;

        assert_eq!((outcome.sessions, outcome.dropped.total()), (1, 0));
        let started: Vec<i64> = sessions_named(&root)
            .await
            .into_iter()
            .map(|session| session.started_at)
            .collect();
        assert_eq!(started, [delayed.departure.timestamp_millis()]);
    }

    #[tokio::test]
    async fn a_trip_whose_legs_overlap_is_left_out_and_counted() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let segment = fixture().remove(0);
        let early = onward_from(&segment, segment.arrival - Duration::minutes(30));
        poll(&root, polled_at(0), &[segment, early]).await;

        let outcome = derive(&root, &germany()).await;

        assert_eq!((outcome.trips, outcome.sessions), (1, 0));
        assert_eq!(
            outcome.dropped,
            DroppedTrips {
                overlapping: 1,
                ..DroppedTrips::default()
            }
        );
    }

    #[tokio::test]
    async fn a_trip_with_a_leg_arriving_before_it_departs_is_left_out_and_counted() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let mut segment = fixture().remove(0);
        segment.arrival = segment.departure - Duration::minutes(1);
        poll(&root, polled_at(0), std::slice::from_ref(&segment)).await;

        let outcome = derive(&root, &germany()).await;

        assert_eq!((outcome.trips, outcome.sessions), (1, 0));
        assert_eq!(
            outcome.dropped,
            DroppedTrips {
                arrives_before_departing: 1,
                ..DroppedTrips::default()
            }
        );
    }

    #[tokio::test]
    async fn a_trip_of_several_legs_waits_at_the_stops_between_them() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let segment = fixture().remove(0);
        let onward = onward_from(&segment, segment.arrival + Duration::minutes(2));
        poll(&root, polled_at(0), &[segment.clone(), onward.clone()]).await;

        let outcome = derive(&root, &germany()).await;

        assert_eq!((outcome.trips, outcome.sessions), (1, 1));
        let samples = (onward.arrival - segment.departure).num_seconds() / 10 + 1;
        assert_eq!(outcome.samples as i64, samples);
    }

    #[tokio::test]
    async fn a_trip_with_a_shapeless_leg_is_left_out_and_counted() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let mut segment = fixture().remove(0);
        segment.polyline = String::new();
        segment.to = segment.from.clone();
        poll(&root, polled_at(0), std::slice::from_ref(&segment)).await;

        let outcome = derive(&root, &germany()).await;

        assert_eq!((outcome.trips, outcome.sessions), (1, 0));
        assert_eq!(
            outcome.dropped,
            DroppedTrips {
                shapeless: 1,
                ..DroppedTrips::default()
            }
        );
    }

    #[tokio::test]
    async fn a_trip_outside_every_known_country_is_not_written() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        poll(&root, polled_at(0), &fixture()).await;

        let outcome = derive(&root, &Nowhere).await;

        assert_eq!(outcome.sessions, 0);
        assert_eq!(
            outcome.dropped,
            DroppedTrips {
                unplaceable: 4,
                ..DroppedTrips::default()
            }
        );
        assert!(!root.path().join("silver").exists());
    }

    #[tokio::test]
    async fn samples_are_written_under_the_country_their_trip_starts_in() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        poll(&root, polled_at(0), &fixture()).await;

        derive(&root, &germany()).await;

        let countries: Vec<String> =
            std::fs::read_dir(root.path().join("silver/train_session_sample"))
                .expect("dataset dir")
                .map(|entry| {
                    entry
                        .expect("entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
        assert_eq!(countries, ["country=DE"]);
    }

    #[tokio::test]
    async fn re_deriving_is_idempotent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        poll(&root, polled_at(0), &fixture()).await;
        let first = derive(&root, &germany()).await;
        let samples_before = count(&root, "SELECT COUNT(*) AS count FROM samples").await;

        let second = derive(&root, &germany()).await;

        assert_eq!(first.samples, second.samples);
        assert_eq!(
            count(&root, "SELECT COUNT(*) AS count FROM samples").await,
            samples_before
        );
    }
}
