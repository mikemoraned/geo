use chrono::{DateTime, Utc};
use domain::Bbox;
use geo::{BoundingRect, Distance, Euclidean};
use geo_types::{LineString, Point};
use medallion::{Countries, GeoRow, Projector, Replaced, Root};
use medallion_model::{SessionRow, SessionSampleRow};

use crate::sessions::Session;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WriteOutcome {
    pub sessions: usize,
    pub session_partitions: Replaced,
    pub samples: usize,
    pub sample_partitions: Replaced,
    pub unplaceable: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum SilverError {
    #[error("geometry: {0}")]
    Geo(#[from] medallion::GeoError),
    #[error("writing the datasets: {0}")]
    Write(#[from] medallion::TableError),
}

struct Placed {
    row: SessionRow,
    path: LineString<f64>,
    samples: Vec<Located>,
}

struct Located {
    row: SessionSampleRow,
    point: Point<f64>,
}

pub async fn write(
    root: &Root,
    sessions: &[Session],
    countries: &impl Countries,
) -> Result<WriteOutcome, SilverError> {
    let mut outcome = WriteOutcome::default();
    let mut session_rows: Vec<GeoRow<SessionRow, LineString<f64>>> = Vec::new();
    let mut sample_rows: Vec<GeoRow<SessionSampleRow, Point<f64>>> = Vec::new();

    for session in sessions {
        match countries.containing(session.started_from()) {
            None => outcome.unplaceable += 1,
            Some(country) => {
                let placed = place(session, &Projector::for_country(country)?)?;
                sample_rows.extend(placed.samples.into_iter().map(|sample| GeoRow {
                    row: sample.row,
                    geometry: sample.point,
                    country,
                }));
                session_rows.push(GeoRow {
                    row: placed.row,
                    geometry: placed.path,
                    country,
                });
            }
        }
    }

    outcome.sessions = session_rows.len();
    outcome.samples = sample_rows.len();
    outcome.session_partitions = medallion::write_geo_rows(root, &session_rows)
        .await?
        .partitions;
    outcome.sample_partitions = medallion::write_geo_rows(root, &sample_rows)
        .await?
        .partitions;
    Ok(outcome)
}

fn place(session: &Session, projector: &Projector) -> Result<Placed, medallion::GeoError> {
    let samples = locate(session, projector)?;
    let path = path_through(samples.iter().map(|sample| sample.point));

    let row = SessionRow {
        session_id: session.id(),
        device_id: session.device_id.clone(),
        started_at: session.started_at(),
        ended_at: samples
            .last()
            .expect("a session is built from the sample that starts it")
            .row
            .t,
        sample_count: samples.len().try_into().unwrap_or(u32::MAX),
        started_by: session.started_by,
        gap_seconds: session.gap.as_seconds(),
        lead_seconds: session.lead.as_seconds(),
        bbox: envelope(&path),
    };

    Ok(Placed { row, path, samples })
}

fn path_through(points: impl Iterator<Item = Point<f64>>) -> LineString<f64> {
    let coords: Vec<_> = points.map(|point| point.0).collect();
    match coords.as_slice() {
        [only] => LineString::new(vec![*only, *only]),
        _ => LineString::new(coords),
    }
}

fn envelope(path: &LineString<f64>) -> Bbox {
    Bbox::of(
        path.bounding_rect()
            .expect("a path holds at least one coordinate"),
    )
}

fn locate(session: &Session, projector: &Projector) -> Result<Vec<Located>, medallion::GeoError> {
    let session_id = session.id();
    let mut located: Vec<Located> = Vec::with_capacity(session.samples.len());
    let mut previous: Option<(Point<f64>, DateTime<Utc>)> = None;

    for (seq, sample) in session.samples.iter().enumerate() {
        let point = Point::new(sample.lon, sample.lat);
        let projected = projector.project(&point)?;
        let row = SessionSampleRow {
            session_id: session_id.clone(),
            device_id: session.device_id.clone(),
            t: sample.t,
            seq: seq.try_into().unwrap_or(u32::MAX),
            lat: sample.lat,
            lon: sample.lon,
            alt: sample.alt,
            acc: sample.acc,
            speed: sample.speed,
            heading: sample.heading,
            implied_speed_mps: previous
                .map(|previous| implied_speed(previous, (projected, sample.t))),
        };
        located.push(Located { row, point });
        previous = Some((projected, sample.t));
    }

    Ok(located)
}

fn implied_speed(from: (Point<f64>, DateTime<Utc>), to: (Point<f64>, DateTime<Utc>)) -> f64 {
    let seconds = (to.1 - from.1).num_milliseconds() as f64 / 1_000.0;
    Euclidean.distance(from.0, to.0) / seconds
}

#[cfg(test)]
mod tests {
    use arrow::array::RecordBatch;
    use chrono::{Duration, TimeZone};
    use domain::Gps;
    use domain::{DeviceId, SessionId};
    use medallion::{Country, GEOMETRY, PROJECTED_GEOMETRY, Query};
    use serde::Deserialize;
    use shared::{GpsReading, Message, V1Message};
    use uuid::Uuid;

    use crate::bronze::{Archive, Payload};
    use crate::sessions::{Gap, Lead, sessions};

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

    const METRES_PER_DEGREE_LATITUDE: f64 = 111_320.0;

    fn at(hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 26, hour, minute, second)
            .unwrap()
    }

    fn gps(id: Uuid, t: DateTime<Utc>, lat: f64, lon: f64) -> Message {
        Message::Version1(V1Message::Gps(GpsReading {
            id,
            t: t.timestamp_millis(),
            gps: Gps::at(lat, lon)
                .expect("on the globe")
                .with_altitude_metres(Some(38.0))
                .with_accuracy_metres(Some(5.0))
                .with_speed_mps(Some(27.0))
                .with_heading_degrees(Some(91.0)),
        }))
    }

    #[derive(Debug, Deserialize)]
    struct Written {
        session_id: SessionId,
        device_id: DeviceId,
        seq: u32,
        implied_speed_mps: Option<f64>,
    }

    async fn drain(root: &Root, messages: &[Message]) {
        let json: Vec<String> = messages
            .iter()
            .map(|message| serde_json::to_string(message).expect("serialize"))
            .collect();
        let payloads: Vec<Payload> = json
            .iter()
            .map(|json| Payload {
                received_at: Some(at(9, 0, 0).timestamp_millis()),
                json,
            })
            .collect();
        Archive::new(root.clone())
            .write(at(9, 0, 0), &payloads)
            .await
            .expect("archive");
    }

    async fn written(tmp: &tempfile::TempDir, messages: &[Message]) -> (Root, WriteOutcome) {
        let root = Root::new(tmp.path());
        drain(&root, messages).await;

        let derived = sessions(&root, Gap::default(), Lead::default())
            .await
            .expect("derive sessions");
        let outcome = write(&root, &derived, &germany())
            .await
            .expect("write sessions");
        (root, outcome)
    }

    async fn dataset(root: &Root) -> Query {
        let query = Query::new(root.clone());
        query
            .register(medallion_model::SESSION_SAMPLE, "samples")
            .await
            .expect("register");
        query
    }

    async fn rows(root: &Root) -> Vec<Written> {
        dataset(root)
            .await
            .rows("SELECT session_id, device_id, seq, implied_speed_mps FROM samples ORDER BY t")
            .await
            .expect("query samples")
    }

    fn first_line(batch: &RecordBatch, column: &str) -> Vec<(f64, f64)> {
        let geometries = medallion::geometries(batch, column).expect("geometries");
        let geo_types::Geometry::LineString(line) = &geometries[0] else {
            panic!("{column} should hold a LineString");
        };
        line.coords().map(|coord| (coord.x, coord.y)).collect()
    }

    fn first_point(batch: &RecordBatch, column: &str) -> (f64, f64) {
        let geometries = medallion::geometries(batch, column).expect("geometries");
        let geo_types::Geometry::Point(point) = &geometries[0] else {
            panic!("{column} should hold a Point");
        };
        (point.x(), point.y())
    }

    #[tokio::test]
    async fn every_sample_becomes_a_row_of_its_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);

        let (root, outcome) = written(
            &tmp,
            &[
                gps(id, at(9, 0, 0), 52.5, 13.4),
                gps(id, at(9, 0, 10), 52.6, 13.4),
                gps(id, at(9, 0, 20), 52.7, 13.4),
            ],
        )
        .await;

        assert_eq!(outcome.samples, 3);
        let rows = rows(&root).await;
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].device_id, DeviceId::from(id));
        assert_eq!(
            rows.iter().map(|row| row.seq).collect::<Vec<_>>(),
            [0, 1, 2],
            "seq counts a sample's place in its session"
        );
        assert_eq!(
            rows.iter()
                .map(|row| row.session_id.clone())
                .collect::<Vec<_>>(),
            vec![rows[0].session_id.clone(); 3],
            "one session's samples all carry its id"
        );
    }

    #[tokio::test]
    async fn a_sample_carries_the_derived_id_of_its_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);

        let (root, _) = written(&tmp, &[gps(id, at(9, 0, 0), 52.5, 13.4)]).await;

        assert_eq!(
            rows(&root).await[0].session_id,
            SessionId::of(&DeviceId::from(id), at(9, 0, 0))
        );
    }

    #[tokio::test]
    async fn implied_speed_is_metres_per_second_over_the_previous_sample() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);

        let (root, _) = written(
            &tmp,
            &[
                gps(id, at(9, 0, 0), 52.5, 13.4),
                gps(id, at(9, 0, 10), 52.6, 13.4),
            ],
        )
        .await;

        let rows = rows(&root).await;
        assert_eq!(
            rows[0].implied_speed_mps, None,
            "the first sample has no previous one to imply a speed"
        );
        let implied = rows[1].implied_speed_mps.expect("a speed");
        let expected = 0.1 * METRES_PER_DEGREE_LATITUDE / 10.0;
        assert!(
            (implied - expected).abs() / expected < 0.01,
            "expected about {expected} m/s, got {implied}"
        );
    }

    #[tokio::test]
    async fn a_session_crossing_midnight_is_written_to_a_partition_per_date() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);
        let midnight = Utc.with_ymd_and_hms(2026, 7, 27, 0, 0, 0).unwrap();

        let (root, outcome) = written(
            &tmp,
            &[
                gps(id, midnight - Duration::minutes(1), 52.5, 13.4),
                gps(id, midnight + Duration::minutes(1), 52.6, 13.4),
            ],
        )
        .await;

        assert_eq!(outcome.sample_partitions.written, 2);
        let rows = rows(&root).await;
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].session_id, rows[1].session_id,
            "one session, whichever date its samples fall on"
        );
    }

    #[tokio::test]
    async fn the_geometry_columns_hold_the_position_in_degrees_and_in_metres() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);

        let (root, _) = written(&tmp, &[gps(id, at(9, 0, 0), 52.520008, 13.404954)]).await;

        let batches = dataset(&root)
            .await
            .sql(&format!(
                "SELECT ST_AsBinary({GEOMETRY}) AS {GEOMETRY},
                        ST_AsBinary({PROJECTED_GEOMETRY}) AS {PROJECTED_GEOMETRY}
                 FROM samples"
            ))
            .await
            .expect("query geometries");

        assert_eq!(first_point(&batches[0], GEOMETRY), (13.404954, 52.520008));
        let (easting, northing) = first_point(&batches[0], PROJECTED_GEOMETRY);
        assert!(
            (easting - 798_809.63).abs() < 0.01 && (northing - 5_828_000.60).abs() < 0.01,
            "expected metres in the German zone, got {easting}, {northing}"
        );
    }

    #[tokio::test]
    async fn writing_again_replaces_the_partition() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);
        let messages = [
            gps(id, at(9, 0, 0), 52.5, 13.4),
            gps(id, at(9, 0, 10), 52.6, 13.4),
        ];

        let (root, first) = written(&tmp, &messages).await;
        let derived = sessions(&root, Gap::default(), Lead::default())
            .await
            .expect("derive sessions");
        let second = write(&root, &derived, &germany())
            .await
            .expect("write again");

        assert_eq!(first, second);
        assert_eq!(rows(&root).await.len(), 2);
    }

    #[derive(Debug, Deserialize)]
    struct WrittenSession {
        session_id: SessionId,
        device_id: DeviceId,
        #[serde(with = "chrono::serde::ts_milliseconds")]
        started_at: DateTime<Utc>,
        #[serde(with = "chrono::serde::ts_milliseconds")]
        ended_at: DateTime<Utc>,
        sample_count: u32,
        started_by: String,
        gap_seconds: u32,
        bbox: Bbox,
    }

    async fn session_rows(root: &Root) -> Vec<WrittenSession> {
        let query = Query::new(root.clone());
        query
            .register(medallion_model::SESSION, "sessions")
            .await
            .expect("register");
        query
            .rows(
                "SELECT session_id, device_id, started_at, ended_at, sample_count, started_by,
                        gap_seconds, bbox
                 FROM sessions ORDER BY started_at",
            )
            .await
            .expect("query sessions")
    }

    #[tokio::test]
    async fn a_session_row_spans_its_samples() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);

        let (root, outcome) = written(
            &tmp,
            &[
                gps(id, at(9, 0, 0), 52.5, 13.4),
                gps(id, at(9, 0, 10), 52.6, 13.5),
                gps(id, at(9, 0, 20), 52.4, 13.3),
            ],
        )
        .await;

        assert_eq!(outcome.sessions, 1);
        let sessions = session_rows(&root).await;
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.device_id, DeviceId::from(id));
        assert_eq!(session.started_at, at(9, 0, 0));
        assert_eq!(session.ended_at, at(9, 0, 20));
        assert_eq!(session.sample_count, 3);
        assert_eq!(session.started_by, "first_seen");
        assert_eq!(session.gap_seconds, 600);
        assert_eq!(
            session.bbox,
            Bbox::new(13.3, 52.4, 13.5, 52.6).expect("a window"),
            "the envelope covers every sample, in lat/lon"
        );
    }

    #[tokio::test]
    async fn a_session_and_its_samples_share_an_id() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);

        let (root, _) = written(
            &tmp,
            &[
                gps(id, at(9, 0, 0), 52.5, 13.4),
                gps(id, at(9, 0, 10), 52.6, 13.4),
            ],
        )
        .await;

        assert_eq!(
            session_rows(&root).await[0].session_id,
            rows(&root).await[0].session_id
        );
    }

    #[tokio::test]
    async fn a_session_crossing_midnight_is_one_row_under_its_start_date() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);
        let midnight = Utc.with_ymd_and_hms(2026, 7, 27, 0, 0, 0).unwrap();

        let (root, outcome) = written(
            &tmp,
            &[
                gps(id, midnight - Duration::minutes(1), 52.5, 13.4),
                gps(id, midnight + Duration::minutes(1), 52.6, 13.4),
            ],
        )
        .await;

        assert_eq!(outcome.session_partitions.written, 1);
        assert_eq!(outcome.sample_partitions.written, 2);
        assert!(
            root.path()
                .join("silver/session/country=DE/start_date=2026-07-26")
                .exists()
        );
    }

    #[tokio::test]
    async fn the_path_columns_hold_the_line_through_the_samples() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);

        let (root, _) = written(
            &tmp,
            &[
                gps(id, at(9, 0, 0), 52.520008, 13.404954),
                gps(id, at(9, 0, 10), 52.6, 13.5),
            ],
        )
        .await;

        let query = Query::new(root.clone());
        query
            .register(medallion_model::SESSION, "sessions")
            .await
            .expect("register");
        let batches = query
            .sql(&format!(
                "SELECT ST_AsBinary({GEOMETRY}) AS {GEOMETRY},
                        ST_AsBinary({PROJECTED_GEOMETRY}) AS {PROJECTED_GEOMETRY}
                 FROM sessions"
            ))
            .await
            .expect("query geometries");

        let line = first_line(&batches[0], GEOMETRY);
        assert_eq!(line[0], (13.404954, 52.520008));
        assert_eq!(line.len(), 2);
        let projected = first_line(&batches[0], PROJECTED_GEOMETRY);
        assert!(
            (projected[0].0 - 798_809.63).abs() < 0.01,
            "expected metres in the German zone, got {:?}",
            projected[0]
        );
    }

    #[tokio::test]
    async fn a_session_of_one_sample_has_a_path_that_stands_still() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);

        let (root, _) = written(&tmp, &[gps(id, at(9, 0, 0), 52.5, 13.4)]).await;

        let query = Query::new(root.clone());
        query
            .register(medallion_model::SESSION, "sessions")
            .await
            .expect("register");
        let batches = query
            .sql(&format!(
                "SELECT ST_AsBinary({GEOMETRY}) AS {GEOMETRY} FROM sessions"
            ))
            .await
            .expect("query geometries");

        assert_eq!(
            first_line(&batches[0], GEOMETRY),
            [(13.4, 52.5), (13.4, 52.5)]
        );
    }

    #[tokio::test]
    async fn a_partition_a_rerun_no_longer_produces_is_removed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);
        let midnight = Utc.with_ymd_and_hms(2026, 7, 27, 0, 0, 0).unwrap();

        let an_hour_apart_over_midnight = [
            gps(id, midnight - Duration::minutes(30), 52.5, 13.4),
            gps(id, midnight + Duration::minutes(30), 52.6, 13.4),
        ];

        let (root, first) = written(&tmp, &an_hour_apart_over_midnight).await;

        assert_eq!(first.sessions, 2, "one session starting on each date");
        assert_eq!(first.session_partitions.written, 2);

        let longer_than_the_silence = Gap::new(Duration::hours(2));
        let derived = sessions(&root, longer_than_the_silence, Lead::default())
            .await
            .expect("derive sessions");
        let second = write(&root, &derived, &germany())
            .await
            .expect("write again");

        assert_eq!(second.sessions, 1);
        assert_eq!(second.session_partitions.written, 1);
        assert_eq!(second.session_partitions.removed, 1);
        assert!(
            !root
                .path()
                .join("silver/session/country=DE/start_date=2026-07-27")
                .exists()
        );
        assert_eq!(session_rows(&root).await.len(), 1);
    }

    #[tokio::test]
    async fn a_session_outside_every_known_country_is_not_written() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);
        let root = Root::new(tmp.path());
        drain(&root, &[gps(id, at(9, 0, 0), 52.5, 13.4)]).await;
        let derived = sessions(&root, Gap::default(), Lead::default())
            .await
            .expect("derive sessions");

        let outcome = write(&root, &derived, &Nowhere).await.expect("write");

        assert_eq!(
            outcome,
            WriteOutcome {
                unplaceable: 1,
                ..WriteOutcome::default()
            }
        );
        assert!(!root.path().join("silver").exists());
    }

    #[tokio::test]
    async fn a_session_is_written_under_the_country_it_started_in() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);

        let (root, _) = written(&tmp, &[gps(id, at(9, 0, 0), 52.5, 13.4)]).await;

        assert!(
            root.path()
                .join("silver/session/country=DE/start_date=2026-07-26")
                .exists()
        );
        assert!(
            root.path()
                .join("silver/session_sample/country=DE/sample_date=2026-07-26")
                .exists()
        );
    }

    #[tokio::test]
    async fn a_country_a_rerun_derives_nothing_in_is_removed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);
        let (root, first) = written(&tmp, &[gps(id, at(9, 0, 0), 52.5, 13.4)]).await;
        assert_eq!(first.sessions, 1);
        assert!(root.path().join("silver/session/country=DE").exists());

        let derived = sessions(&root, Gap::default(), Lead::default())
            .await
            .expect("derive sessions");
        let outcome = write(&root, &derived, &Nowhere).await.expect("write again");

        assert_eq!(outcome.unplaceable, 1);
        assert_eq!(outcome.session_partitions.removed, 1);
        assert_eq!(outcome.sample_partitions.removed, 1);
        assert!(!root.path().join("silver/session/country=DE").exists());
        assert!(
            !root
                .path()
                .join("silver/session_sample/country=DE")
                .exists()
        );
    }

    #[tokio::test]
    async fn no_sessions_write_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let outcome = write(&Root::new(tmp.path()), &[], &germany())
            .await
            .expect("write nothing");

        assert_eq!(outcome, WriteOutcome::default());
        assert!(!tmp.path().join("silver").exists());
    }
}
