//! Reading the pre-medallion sqlite capture log into the bronze capture log.
//!
//! The old log holds one row per polled segment in a `segment` table, each row carrying the
//! `captured_at` of the poll that saw it. Bronze keeps a poll as a file, so the rows are
//! grouped back into the polls they came from and written one file per poll — the same shape
//! [`SegmentLog::append`](crate::bronze::SegmentLog::append) produces live.
//!
//! A poll whose file is already there is left alone, so a run adds only what is missing and
//! never rewrites an immutable file — which also makes a repeated run a no-op.

use std::path::Path;

use chrono::{DateTime, Utc};
use medallion::Root;

use model::MotisSegmentRow;

use crate::bronze::{BronzeError, SegmentLog};

/// The logged segments, oldest poll first, so a partial run has covered a prefix of the
/// history rather than an arbitrary subset.
const SEGMENTS: &str = "
    SELECT captured_at, trip_id, route_name, train_number, agency_id, agency_name, mode,
           route_color, from_stop_id, from_lat, from_lon, to_stop_id, to_lat, to_lon,
           departure, arrival, scheduled_departure, scheduled_arrival, realtime, polyline
    FROM segment
    ORDER BY captured_at, rowid
";

/// A failure backfilling.
#[derive(Debug, thiserror::Error)]
pub enum BackfillError {
    #[error("opening {path}: {source}")]
    Open {
        path: String,
        source: rusqlite::Error,
    },
    #[error("reading the capture log: {0}")]
    Read(#[from] rusqlite::Error),
    #[error("locating the poll's file: {0}")]
    Path(#[from] BronzeError),
}

/// A time column holding something no instant can be made of.
#[derive(Debug, thiserror::Error)]
#[error("{millis}ms is not a representable instant")]
struct NotAnInstant {
    millis: i64,
}

/// What a backfill wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Outcome {
    /// Rows read out of the old log.
    pub read: usize,
    /// Polls those rows belong to.
    pub polls: usize,
    /// Rows written, which is `read` less those belonging to skipped polls.
    pub written: usize,
    /// Polls left alone because bronze already held their file.
    pub skipped: usize,
}

/// Read the capture log at `db` into the bronze capture log under `root`.
pub async fn backfill(db: &Path, root: &Root) -> Result<Outcome, BackfillError> {
    let polls = read(db)?;
    let log = SegmentLog::new(root.clone());

    let mut outcome = Outcome {
        polls: polls.len(),
        read: polls.iter().map(|(_, rows)| rows.len()).sum(),
        ..Outcome::default()
    };

    for (captured_at, rows) in &polls {
        if log.poll_file(*captured_at)?.exists() {
            outcome.skipped += 1;
        } else {
            outcome.written += log.append_rows(*captured_at, rows).await?;
        }
    }

    Ok(outcome)
}

/// One poll as the old log holds it: the instant it was made, and the segments it saw.
type Poll = (DateTime<Utc>, Vec<MotisSegmentRow>);

/// The logged rows, grouped into their polls and ordered oldest first.
fn read(db: &Path) -> Result<Vec<Poll>, BackfillError> {
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|source| BackfillError::Open {
                path: db.display().to_string(),
                source,
            })?;

    let mut statement = conn.prepare(SEGMENTS)?;
    let rows = statement.query_map([], |row| {
        Ok(MotisSegmentRow {
            captured_at: instant(row.get(0)?)?,
            trip_id: row.get(1)?,
            route_name: row.get(2)?,
            train_number: row.get(3)?,
            agency_id: row.get(4)?,
            agency_name: row.get(5)?,
            mode: row.get(6)?,
            route_color: row.get(7)?,
            from_stop_id: row.get(8)?,
            from_lat: row.get(9)?,
            from_lon: row.get(10)?,
            to_stop_id: row.get(11)?,
            to_lat: row.get(12)?,
            to_lon: row.get(13)?,
            departure: instant(row.get(14)?)?,
            arrival: instant(row.get(15)?)?,
            scheduled_departure: instant(row.get(16)?)?,
            scheduled_arrival: instant(row.get(17)?)?,
            realtime: row.get(18)?,
            polyline: row.get(19)?,
        })
    })?;

    // Rows arrive ordered by poll, so each change of `captured_at` starts a new poll.
    let mut polls: Vec<Poll> = Vec::new();
    for row in rows {
        let segment = row?;
        let captured_at = segment.captured_at;
        match polls.last_mut() {
            Some((poll, segments)) if *poll == captured_at => segments.push(segment),
            _ => polls.push((captured_at, vec![segment])),
        }
    }
    Ok(polls)
}

/// An epoch-millis column as the instant it stands for. The old log stores every time this
/// way; a value outside the representable range is a corrupt row, not a time.
fn instant(millis: i64) -> Result<DateTime<Utc>, rusqlite::Error> {
    DateTime::from_timestamp_millis(millis).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(NotAnInstant { millis }),
        )
    })
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use medallion::Query;

    use super::*;

    fn captured(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 20, hour, minute, 0).unwrap()
    }

    /// A log holding one row per `(captured_at, trip_id)` given.
    fn log_db(dir: &Path, rows: &[(DateTime<Utc>, &str)]) -> std::path::PathBuf {
        let path = dir.join("motis.sqlite");
        let conn = rusqlite::Connection::open(&path).expect("create log");
        conn.execute(
            "CREATE TABLE segment (
               rowid INTEGER PRIMARY KEY, captured_at INTEGER NOT NULL, trip_id TEXT NOT NULL,
               route_name TEXT, mode TEXT NOT NULL, route_color TEXT, from_stop_id TEXT,
               from_lat REAL NOT NULL, from_lon REAL NOT NULL, to_stop_id TEXT,
               to_lat REAL NOT NULL, to_lon REAL NOT NULL, departure INTEGER NOT NULL,
               arrival INTEGER NOT NULL, scheduled_departure INTEGER NOT NULL,
               scheduled_arrival INTEGER NOT NULL, realtime INTEGER NOT NULL,
               polyline TEXT NOT NULL, agency_id TEXT, agency_name TEXT, train_number INTEGER
             )",
            [],
        )
        .expect("create segment");
        for (captured_at, trip_id) in rows {
            conn.execute(
                "INSERT INTO segment (captured_at, trip_id, route_name, mode, route_color,
                                      from_stop_id, from_lat, from_lon, to_stop_id, to_lat,
                                      to_lon, departure, arrival, scheduled_departure,
                                      scheduled_arrival, realtime, polyline, train_number)
                 VALUES (?1, ?2, 'RE4', 'REGIONAL_RAIL', NULL, 'from', 50.1, 8.6, 'to', 50.2,
                         8.7, ?3, ?4, ?3, ?4, 1, '_p~iF~ps|U_ulLnnqC', 2569)",
                rusqlite::params![
                    captured_at.timestamp_millis(),
                    trip_id,
                    captured_at.timestamp_millis(),
                    captured_at.timestamp_millis() + 600_000,
                ],
            )
            .expect("insert segment");
        }
        path
    }

    async fn rows_in(root: &Root) -> i64 {
        let query = Query::new(root.clone());
        query
            .register(model::MOTIS_SEGMENT, "d")
            .await
            .expect("register");
        query
            .count("SELECT COUNT(*) AS count FROM d")
            .await
            .expect("count")
    }

    #[tokio::test]
    async fn each_logged_poll_becomes_one_bronze_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path().join("store"));
        let db = log_db(
            tmp.path(),
            &[
                (captured(9, 0), "trip-a"),
                (captured(9, 0), "trip-b"),
                (captured(9, 1), "trip-a"),
            ],
        );

        let outcome = backfill(&db, &root).await.expect("backfill");

        assert_eq!(
            outcome,
            Outcome {
                read: 3,
                polls: 2,
                written: 3,
                skipped: 0
            }
        );
        assert_eq!(rows_in(&root).await, 3);
        let poll_files: Vec<_> = std::fs::read_dir(
            root.path()
                .join("bronze/motis_segment/polled_date=2026-07-20"),
        )
        .expect("partition")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
        assert_eq!(poll_files.len(), 2, "{poll_files:?}");
    }

    /// The row's own columns survive the round trip, not just its count.
    #[tokio::test]
    async fn a_row_keeps_its_columns_and_its_polyline_verbatim() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path().join("store"));
        let db = log_db(tmp.path(), &[(captured(9, 0), "trip-a")]);

        backfill(&db, &root).await.expect("backfill");

        let query = Query::new(root.clone());
        query
            .register(model::MOTIS_SEGMENT, "d")
            .await
            .expect("register");
        let rows: Vec<Logged> = query
            .rows("SELECT trip_id, train_number, polyline, realtime, captured_at, departure FROM d")
            .await
            .expect("query");

        assert_eq!(
            rows,
            vec![Logged {
                trip_id: "trip-a".into(),
                train_number: Some(2569),
                polyline: "_p~iF~ps|U_ulLnnqC".into(),
                realtime: true,
                captured_at: captured(9, 0),
                departure: captured(9, 0),
            }]
        );
    }

    /// The columns of a backfilled row, as bronze holds them.
    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Logged {
        trip_id: String,
        train_number: Option<u32>,
        polyline: String,
        realtime: bool,
        #[serde(with = "chrono::serde::ts_milliseconds")]
        captured_at: DateTime<Utc>,
        #[serde(with = "chrono::serde::ts_milliseconds")]
        departure: DateTime<Utc>,
    }

    /// Bronze files are immutable, so a poll already logged is left as it is — which makes
    /// a repeated run add nothing.
    #[tokio::test]
    async fn a_poll_already_in_bronze_is_skipped() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path().join("store"));
        let db = log_db(tmp.path(), &[(captured(9, 0), "trip-a")]);
        backfill(&db, &root).await.expect("first run");

        let outcome = backfill(&db, &root).await.expect("second run");

        assert_eq!(
            outcome,
            Outcome {
                read: 1,
                polls: 1,
                written: 0,
                skipped: 1
            }
        );
        assert_eq!(rows_in(&root).await, 1);
    }

    #[tokio::test]
    async fn an_empty_log_writes_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path().join("store"));
        let db = log_db(tmp.path(), &[]);

        let outcome = backfill(&db, &root).await.expect("backfill");

        assert_eq!(outcome, Outcome::default());
        assert!(!root.path().join("bronze").exists());
    }
}
