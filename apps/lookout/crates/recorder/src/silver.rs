//! Writing derived sessions into silver: a session's path and envelope, and a row per
//! sample within it.
//!
//! Every geometry is carried twice: in lat/lon, and projected into metres. The projected
//! one is what makes a distance a distance — an implied speed is metres per second, and
//! degrees are neither metres nor the same size in both axes.
//!
//! The two datasets are partitioned by different dates — a sample by its own instant, a
//! session by the instant it began — so a session crossing midnight has its samples split
//! over two partitions while itself living in one. A run rebuilds every partition it
//! produces rows for, since it re-derives every session from all of bronze.
//!
//! Both sit under a `country=` partition, because the projected column's CRS is declared
//! per file and the zone is chosen per country: rows of two countries cannot share a file
//! and state one CRS truthfully. Which country a session is in follows from where it
//! started, so a session whose start is in no country the store knows is left unwritten —
//! there is no zone to project it into — and counted.

use arrow::array::RecordBatch;
use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use geo::{BoundingRect, Distance, Euclidean};
use geo_types::{LineString, Point};
use medallion::{Countries, Country, Projector, Replaced, Root, Row, GEOMETRY, PROJECTED_GEOMETRY};
use model::{Bbox, SessionRow, SessionSampleRow};

use crate::sessions::Session;

/// The partition key each dataset is laid out by above its date, since a file's projected
/// geometry states one CRS and the zone is chosen per country.
const COUNTRY: &str = "country";

/// What one write did, per dataset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WriteOutcome {
    pub sessions: usize,
    pub session_partitions: Replaced,
    pub samples: usize,
    pub sample_partitions: Replaced,
    /// Sessions starting outside every country the store knows, and so not written.
    pub unplaceable: usize,
}

/// A failure writing the sessions.
#[derive(Debug, thiserror::Error)]
pub enum SilverError {
    #[error("geometry: {0}")]
    Geo(#[from] medallion::GeoError),
    #[error("partitioning the dataset: {0}")]
    Path(#[from] medallion::PathError),
    #[error("replacing the partitions: {0}")]
    Replace(#[from] medallion::ReplaceError),
}

/// One session's row, the path its geometry columns hold, and its samples.
struct Placed {
    row: SessionRow,
    path: LineString<f64>,
    projected_path: LineString<f64>,
    samples: Vec<Located>,
}

/// One sample's row, with the points its geometry columns hold.
struct Located {
    row: SessionSampleRow,
    point: Point<f64>,
    projected: Point<f64>,
}

/// Write `sessions` and their samples to the silver datasets under `root`.
///
/// Each session's country is looked up from where it started, since that fixes the CRS of
/// its projected geometry. A session starting where `countries` knows no country is
/// reported as unplaceable rather than written.
pub async fn write(
    root: &Root,
    sessions: &[Session],
    countries: &impl Countries,
) -> Result<WriteOutcome, SilverError> {
    let mut outcome = WriteOutcome::default();
    let mut by_country: HashMap<Country, Vec<&Session>> = HashMap::new();
    for session in sessions {
        match countries.containing(session.started_from()) {
            Some(country) => by_country.entry(country).or_default().push(session),
            None => outcome.unplaceable += 1,
        }
    }

    for (country, sessions) in by_country {
        let projector = Projector::for_country(country)?;
        let placed = sessions
            .iter()
            .map(|session| place(session, &projector))
            .collect::<Result<Vec<_>, _>>()?;

        outcome.sessions += placed.len();
        outcome.samples += placed
            .iter()
            .map(|session| session.samples.len())
            .sum::<usize>();
        outcome.session_partitions += write_sessions(root, &placed, country).await?;
        outcome.sample_partitions += write_samples(root, &placed, country).await?;
    }

    Ok(outcome)
}

/// Write one row per session, partitioned by the date it began.
async fn write_sessions(
    root: &Root,
    placed: &[Placed],
    country: Country,
) -> Result<Replaced, SilverError> {
    let mut placed: Vec<&Placed> = placed.iter().collect();
    placed.sort_by_key(|session| session.row.started_at);

    write_dates::<_, SessionRow, _, _>(
        root,
        country,
        &placed,
        |session| session.row.started_at.date_naive(),
        |day| {
            let rows: Vec<SessionRow> = day.iter().map(|session| session.row.clone()).collect();
            let paths: Vec<LineString<f64>> =
                day.iter().map(|session| session.path.clone()).collect();
            let projected: Vec<LineString<f64>> = day
                .iter()
                .map(|session| session.projected_path.clone())
                .collect();
            batch(&rows, &paths, &projected, country)
        },
    )
    .await
}

/// Write one row per sample, partitioned by the date of the sample itself.
async fn write_samples(
    root: &Root,
    placed: &[Placed],
    country: Country,
) -> Result<Replaced, SilverError> {
    // Sessions run concurrently across devices and cross midnight, so the samples are
    // ordered by instant to gather each date's into one adjacent run.
    let mut samples: Vec<&Located> = placed
        .iter()
        .flat_map(|session| session.samples.iter())
        .collect();
    samples.sort_by_key(|sample| sample.row.t);

    write_dates::<_, SessionSampleRow, _, _>(
        root,
        country,
        &samples,
        |sample| sample.row.t.date_naive(),
        |day| {
            let rows: Vec<SessionSampleRow> = day.iter().map(|sample| sample.row.clone()).collect();
            let points: Vec<Point<f64>> = day.iter().map(|sample| sample.point).collect();
            let projected: Vec<Point<f64>> = day.iter().map(|sample| sample.projected).collect();
            batch(&rows, &points, &projected, country)
        },
    )
    .await
}

/// Replace one country's partitions with one per date the rows fall on, reporting what
/// that left in the store.
///
/// `rows` are ordered by the date `date_of` reads, so each partition's rows are one
/// adjacent run rather than a scan of all of them.
async fn write_dates<T, R, D, B>(
    root: &Root,
    country: Country,
    rows: &[&T],
    date_of: D,
    batch_of: B,
) -> Result<Replaced, SilverError>
where
    R: Row,
    D: Fn(&T) -> NaiveDate,
    B: Fn(&[&T]) -> Result<RecordBatch, SilverError>,
{
    let days = rows
        .chunk_by(|a, b| date_of(a) == date_of(b))
        .map(|day| Ok((date_of(day[0]), batch_of(day)?)))
        .collect::<Result<Vec<_>, SilverError>>()?;

    Ok(root
        .rows_of::<R>()
        .partition(COUNTRY, country)?
        .replace_dates_geo(&days)
        .await?)
}

/// One session placed on the map: its row and path, and its samples' rows and points.
fn place(session: &Session, projector: &Projector) -> Result<Placed, medallion::GeoError> {
    let samples = locate(session, projector)?;
    let path = path_through(samples.iter().map(|sample| sample.point));
    let projected_path = path_through(samples.iter().map(|sample| sample.projected));

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

    Ok(Placed {
        row,
        path,
        projected_path,
        samples,
    })
}

/// The path through `points`.
///
/// A session of one sample stands still rather than having no path: its lone point is
/// repeated, so every session's geometry is a line of at least two coordinates and a reader
/// never meets a LineString that simple features would call malformed.
fn path_through(points: impl Iterator<Item = Point<f64>>) -> LineString<f64> {
    let coords: Vec<_> = points.map(|point| point.0).collect();
    match coords.as_slice() {
        [only] => LineString::new(vec![*only, *only]),
        _ => LineString::new(coords),
    }
}

/// The envelope of `path`, in the axis names the upstream reference data uses.
fn envelope(path: &LineString<f64>) -> Bbox {
    let rect = path
        .bounding_rect()
        .expect("a path holds at least one coordinate");
    Bbox {
        xmin: rect.min().x,
        ymin: rect.min().y,
        xmax: rect.max().x,
        ymax: rect.max().y,
    }
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

/// One partition's batch: the rows, then the two geometry columns their positions make.
fn batch<T, G>(
    rows: &[T],
    geometry: &[G],
    projected: &[G],
    country: Country,
) -> Result<RecordBatch, SilverError>
where
    T: Row,
    G: geo_traits::GeometryTrait<T = f64>,
{
    Ok(medallion::geo_batch(
        rows,
        &[
            (medallion::wkb_field(GEOMETRY)?, geometry),
            (
                medallion::projected_wkb_field(PROJECTED_GEOMETRY, country)?,
                projected,
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
    use crate::sessions::{sessions, Gap, Lead};

    use super::*;

    /// Every place is in Germany, which is where these samples are.
    struct Everywhere(Country);

    impl Countries for Everywhere {
        fn containing(&self, _point: Point<f64>) -> Option<Country> {
            Some(self.0)
        }
    }

    /// Nowhere is in any country the store knows.
    struct Nowhere;

    impl Countries for Nowhere {
        fn containing(&self, _point: Point<f64>) -> Option<Country> {
            None
        }
    }

    fn germany() -> Everywhere {
        Everywhere(Country::Germany)
    }

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

    /// The line held in `column` of the first row of `batch`.
    fn first_line(batch: &RecordBatch, column: &str) -> Vec<(f64, f64)> {
        let geometries = medallion::geometries(batch, column).expect("geometries");
        let geo_types::Geometry::LineString(line) = &geometries[0] else {
            panic!("{column} should hold a LineString");
        };
        line.coords().map(|coord| (coord.x, coord.y)).collect()
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
        let derived = sessions(&root, Gap::default(), Lead::default())
            .await
            .expect("derive sessions");
        let second = write(&root, &derived, &germany())
            .await
            .expect("write again");

        assert_eq!(first, second);
        assert_eq!(rows(&root).await.len(), 2);
    }

    /// The columns a reader takes back out of the session dataset.
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
            .register(model::SESSION, "sessions")
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

    /// A session spans its samples: their instants, their count, and the ground they cover.
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
            Bbox {
                xmin: 13.3,
                ymin: 52.4,
                xmax: 13.5,
                ymax: 52.6
            },
            "the envelope covers every sample, in lat/lon"
        );
    }

    /// The id on a session is the one its samples carry, so the two datasets join.
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

    /// A session is one row wherever its samples fall, so a session over midnight lives in
    /// the partition of the date it began.
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
        assert!(root
            .path()
            .join("silver/session/country=DE/start_date=2026-07-26")
            .exists());
    }

    /// The path is the line the session took, in degrees and in metres.
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
            .register(model::SESSION, "sessions")
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

    /// A session of one sample stands still: its path is a line rather than a LineString of
    /// a single coordinate, which simple features would call malformed.
    #[tokio::test]
    async fn a_session_of_one_sample_has_a_path_that_stands_still() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);

        let (root, _) = written(&tmp, &[gps(id, at(9, 0, 0), 52.5, 13.4)]).await;

        let query = Query::new(root.clone());
        query
            .register(model::SESSION, "sessions")
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

    /// The gap threshold moves a session's start into another day, so the partition the
    /// earlier run wrote is no longer produced — and does not survive the run that
    /// replaced it.
    #[tokio::test]
    async fn a_partition_a_rerun_no_longer_produces_is_removed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);
        let midnight = Utc.with_ymd_and_hms(2026, 7, 27, 0, 0, 0).unwrap();

        // Two samples an hour apart over midnight: at the default threshold they are two
        // sessions, one starting on each date.
        let (root, first) = written(
            &tmp,
            &[
                gps(id, midnight - Duration::minutes(30), 52.5, 13.4),
                gps(id, midnight + Duration::minutes(30), 52.6, 13.4),
            ],
        )
        .await;
        assert_eq!(first.sessions, 2);
        assert_eq!(first.session_partitions.written, 2);

        // At a threshold longer than the silence they are one session, starting on the
        // first date only.
        let derived = sessions(&root, Gap::new(Duration::hours(2)), Lead::default())
            .await
            .expect("derive sessions");
        let second = write(&root, &derived, &germany())
            .await
            .expect("write again");

        assert_eq!(second.sessions, 1);
        assert_eq!(second.session_partitions.written, 1);
        assert_eq!(second.session_partitions.removed, 1);
        assert!(!root
            .path()
            .join("silver/session/country=DE/start_date=2026-07-27")
            .exists());
        assert_eq!(session_rows(&root).await.len(), 1);
    }

    /// A session starting where no known country is has no zone to be projected into, so
    /// it is reported rather than written into some other country's metres.
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

    /// The country a session is in decides which zone its geometry is written in, so it
    /// names a partition rather than being a column of the file.
    #[tokio::test]
    async fn a_session_is_written_under_the_country_it_started_in() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = Uuid::from_u128(1);

        let (root, _) = written(&tmp, &[gps(id, at(9, 0, 0), 52.5, 13.4)]).await;

        assert!(root
            .path()
            .join("silver/session/country=DE/start_date=2026-07-26")
            .exists());
        assert!(root
            .path()
            .join("silver/session_sample/country=DE/sample_date=2026-07-26")
            .exists());
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
