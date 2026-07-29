//! Writing derived sessions into silver.
//!
//! A sample's row carries its position twice: in lat/lon, and projected into metres. The
//! projected point is what makes a distance a distance — the speed a step implies is metres
//! per second, and degrees are neither metres nor the same size in both axes.
//!
//! Samples are partitioned by the date of the sample itself, so a session crossing midnight
//! has its samples written to two partitions. A run rebuilds every partition it produces
//! rows for, since it re-derives every session from all of bronze.

use chrono::{DateTime, Utc};
use geo::{Distance, Euclidean};
use geo_types::Point;
use medallion::{Country, Projector, Root, GEOMETRY, PROJECTED_GEOMETRY};
use model::SessionSampleRow;

use crate::sessions::Session;

/// What one write did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WriteOutcome {
    pub rows: usize,
    pub partitions: usize,
}

/// A failure writing the sessions.
#[derive(Debug, thiserror::Error)]
pub enum SilverError {
    #[error("geometry: {0}")]
    Geo(#[from] medallion::GeoError),
    #[error("partitioning the dataset: {0}")]
    Path(#[from] medallion::PathError),
}

/// One sample's row, with the points its geometry columns hold.
struct Located {
    row: SessionSampleRow,
    point: Point<f64>,
    projected: Point<f64>,
}

/// Write `sessions`' samples to the silver dataset under `root`.
///
/// `country` is the one these sessions were recorded in: it fixes the CRS of the projected
/// geometry column, since the projected zone is chosen per country.
pub async fn write_samples(
    root: &Root,
    sessions: &[Session],
    country: Country,
) -> Result<WriteOutcome, SilverError> {
    let projector = Projector::for_country(country)?;
    let mut located = Vec::new();
    for session in sessions {
        located.extend(locate(session, &projector)?);
    }
    // Sessions run concurrently across devices and cross midnight, so the rows are ordered
    // by instant to gather each date's into one adjacent run.
    located.sort_by_key(|sample| sample.row.t);

    let mut outcome = WriteOutcome {
        rows: located.len(),
        partitions: 0,
    };
    for day in located.chunk_by(|a, b| a.row.t.date_naive() == b.row.t.date_naive()) {
        root.rows_of::<SessionSampleRow>()
            .on_date(day[0].row.t.date_naive())?
            .replace_with_geo(&[batch(day, country)?])
            .await?;
        outcome.partitions += 1;
    }

    Ok(outcome)
}

/// One session's samples as rows, each with its position projected.
fn locate(session: &Session, projector: &Projector) -> Result<Vec<Located>, medallion::GeoError> {
    let session_id = session.id();
    let mut located: Vec<Located> = Vec::with_capacity(session.samples.len());

    for (seq, sample) in session.samples.iter().enumerate() {
        let point = Point::new(sample.lon, sample.lat);
        let projected = projector.project(&point)?;
        let previous = located.last();
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
                .map(|previous| implied_speed(previous, projected, sample.t)),
        };
        located.push(Located {
            row,
            point,
            projected,
        });
    }

    Ok(located)
}

/// The speed the step from `previous` to a sample at `t` implies, in metres per second.
///
/// Samples are deduped on `(device_id, t)` before they reach here, so no two samples of a
/// session share an instant and the interval is never zero.
fn implied_speed(previous: &Located, projected: Point<f64>, t: DateTime<Utc>) -> f64 {
    let seconds = (t - previous.row.t).num_milliseconds() as f64 / 1_000.0;
    Euclidean.distance(previous.projected, projected) / seconds
}

/// One partition's batch: the rows, then the geometry columns their positions make.
fn batch(day: &[Located], country: Country) -> Result<arrow::array::RecordBatch, SilverError> {
    let rows: Vec<SessionSampleRow> = day.iter().map(|sample| sample.row.clone()).collect();
    let points: Vec<Point<f64>> = day.iter().map(|sample| sample.point).collect();
    let projected: Vec<Point<f64>> = day.iter().map(|sample| sample.projected).collect();

    Ok(medallion::geo_batch(
        &rows,
        &[
            (medallion::wkb_field(GEOMETRY)?, points.as_slice()),
            (
                medallion::projected_wkb_field(PROJECTED_GEOMETRY, country)?,
                projected.as_slice(),
            ),
        ],
    )?)
}

#[cfg(test)]
mod tests {
    use arrow::array::RecordBatch;
    use chrono::{Duration, TimeZone};
    use medallion::Query;
    use model::{DeviceId, SessionId};
    use serde::Deserialize;
    use shared::{Gps, GpsReading, Message, V1Message};
    use uuid::Uuid;

    use crate::bronze::{Archive, Payload};
    use crate::sessions::{sessions, Gap};

    use super::*;

    /// A degree of latitude is about this many metres, near enough to check that a speed
    /// came out of the projected geometry rather than out of degrees.
    const METRES_PER_DEGREE_LATITUDE: f64 = 111_320.0;

    fn at(hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 26, hour, minute, second)
            .unwrap()
    }

    fn gps(id: Uuid, t: DateTime<Utc>, lat: f64, lon: f64) -> Message {
        Message::Version1(V1Message::Gps(GpsReading {
            id,
            t: t.timestamp_millis(),
            gps: Gps {
                lat,
                lon,
                alt: Some(38.0),
                acc: 5.0,
                speed: Some(27.0),
                heading: Some(91.0),
            },
        }))
    }

    /// The columns a reader takes back out of the dataset.
    #[derive(Debug, Deserialize)]
    struct Written {
        session_id: SessionId,
        device_id: DeviceId,
        seq: u32,
        implied_speed_mps: Option<f64>,
    }

    /// Derive and write the sessions `messages` make, through the same archive the drain
    /// writes bronze with.
    async fn written(tmp: &tempfile::TempDir, messages: &[Message]) -> (Root, WriteOutcome) {
        let root = Root::new(tmp.path());
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

        let derived = sessions(&root, Gap::default())
            .await
            .expect("derive sessions");
        let outcome = write_samples(&root, &derived, Country::Germany)
            .await
            .expect("write samples");
        (root, outcome)
    }

    /// The written dataset, registered the way any other reader would register it.
    async fn dataset(root: &Root) -> Query {
        let query = Query::new(root.clone());
        query
            .register(model::SESSION_SAMPLE, "samples")
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

    /// The point held in `column` of the first row of `batch`.
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

        assert_eq!(outcome.rows, 3);
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

    /// The id is the one derived from what the session is, so a reader can name a session
    /// without reading it back.
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

    /// Implied speed is metres per second: a tenth of a degree of latitude in ten seconds
    /// is about 1.1 km/s, which degrees would have made 0.01.
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

    /// Samples are partitioned by the date of the sample, so a session running over
    /// midnight is written to both dates and reassembled by its id.
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

        assert_eq!(outcome.partitions, 2);
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

    /// A run re-derives every session from all of bronze, so writing again replaces each
    /// partition rather than appending a second copy of it.
    #[tokio::test]
    async fn writing_again_replaces_the_partition() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);
        let messages = [
            gps(id, at(9, 0, 0), 52.5, 13.4),
            gps(id, at(9, 0, 10), 52.6, 13.4),
        ];

        let (root, first) = written(&tmp, &messages).await;
        let derived = sessions(&root, Gap::default())
            .await
            .expect("derive sessions");
        let second = write_samples(&root, &derived, Country::Germany)
            .await
            .expect("write again");

        assert_eq!(first, second);
        assert_eq!(rows(&root).await.len(), 2);
    }

    #[tokio::test]
    async fn no_sessions_write_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let outcome = write_samples(&Root::new(tmp.path()), &[], Country::Germany)
            .await
            .expect("write nothing");

        assert_eq!(outcome, WriteOutcome::default());
        assert!(!tmp.path().join("silver").exists());
    }
}
