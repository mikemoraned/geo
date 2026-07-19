//! The raw `motis` capture log: every `TripSegment` a poll returns, appended verbatim
//! with duplication allowed. There is no unique key — the same scheduled leg re-seen
//! across overlapping polls yields a fresh row each time; dedup happens later in the
//! ingest step. Times are stored as epoch milliseconds — both the realtime-corrected
//! (`departure`/`arrival`) and scheduled (`scheduled_*`) so the delay stays recoverable —
//! and the polyline as its raw Google-encoded string.

use std::path::Path;

use chrono::{DateTime, Utc};
use motis_openapi_progenitor::types::TripSegment;
use rusqlite::{params, Connection};

/// DDL run on open; `IF NOT EXISTS` makes opening an existing db a no-op. No unique key:
/// the log is append-only and duplication is intentional.
const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS segment (
  rowid               INTEGER PRIMARY KEY,
  captured_at         INTEGER NOT NULL,
  trip_id             TEXT NOT NULL,
  route_name          TEXT,
  mode                TEXT NOT NULL,
  route_color         TEXT,
  from_stop_id        TEXT,
  from_lat            REAL NOT NULL,
  from_lon            REAL NOT NULL,
  to_stop_id          TEXT,
  to_lat              REAL NOT NULL,
  to_lon              REAL NOT NULL,
  departure           INTEGER NOT NULL,
  arrival             INTEGER NOT NULL,
  scheduled_departure INTEGER NOT NULL,
  scheduled_arrival   INTEGER NOT NULL,
  realtime            INTEGER NOT NULL,
  polyline            TEXT NOT NULL
);
";

/// Failure persisting to the `motis` capture log.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// A handle to the raw `motis` capture db.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating if absent) the db at `path` and ensure the `segment` table exists.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Open an in-memory db with the schema created; discarded on drop. Test-only.
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Append every segment as its own row, stamped with the `captured_at` poll time.
    /// Always inserts — duplicates are allowed — returning the number of rows written.
    pub fn insert(
        &self,
        captured_at: DateTime<Utc>,
        segments: &[TripSegment],
    ) -> Result<usize, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        for segment in segments {
            insert_row(&tx, captured_at, segment)?;
        }
        tx.commit()?;
        Ok(segments.len())
    }
}

/// Append one segment. Runs on the caller's `conn` (a batch shares one transaction).
fn insert_row(
    conn: &Connection,
    captured_at: DateTime<Utc>,
    segment: &TripSegment,
) -> Result<(), StoreError> {
    let trip = segment.trips.first();
    let trip_id = trip.map(|t| t.trip_id.as_str()).unwrap_or_default();
    let route_name =
        trip.and_then(|t| t.display_name.as_deref().or(t.route_short_name.as_deref()));
    conn.execute(
        "INSERT INTO segment
           (captured_at, trip_id, route_name, mode, route_color,
            from_stop_id, from_lat, from_lon, to_stop_id, to_lat, to_lon,
            departure, arrival, scheduled_departure, scheduled_arrival, realtime, polyline)
         VALUES
           (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            captured_at.timestamp_millis(),
            trip_id,
            route_name,
            segment.mode.to_string(),
            segment.route_color,
            segment.from.stop_id,
            segment.from.lat,
            segment.from.lon,
            segment.to.stop_id,
            segment.to.lat,
            segment.to.lon,
            segment.departure.timestamp_millis(),
            segment.arrival.timestamp_millis(),
            segment.scheduled_departure.timestamp_millis(),
            segment.scheduled_arrival.timestamp_millis(),
            segment.real_time,
            segment.polyline,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The captured real 4-segment, mode-varied fixture (rail/subway/tram/bus).
    fn fixture_segments() -> Vec<TripSegment> {
        serde_json::from_str(include_str!("../tests/fixtures/trips.json"))
            .expect("parse trips fixture")
    }

    fn count(store: &Store) -> i64 {
        store
            .conn
            .query_row("SELECT COUNT(*) FROM segment", [], |r| r.get(0))
            .expect("count")
    }

    #[test]
    fn insert_appends_every_segment() {
        let store = Store::open_in_memory().expect("open");
        let segments = fixture_segments();
        let written = store.insert(Utc::now(), &segments).expect("insert");
        assert_eq!(written, segments.len());
        assert_eq!(count(&store) as usize, segments.len());
    }

    /// Duplication is allowed: the same segment inserted twice yields two rows.
    #[test]
    fn inserting_the_same_segment_twice_yields_two_rows() {
        let store = Store::open_in_memory().expect("open");
        let one = &fixture_segments()[..1];
        store.insert(Utc::now(), one).expect("first");
        store.insert(Utc::now(), one).expect("second");
        assert_eq!(count(&store), 2);
    }

    /// The kept fields round-trip, including both realtime and scheduled times (the
    /// rail fixture carries a delay, so the two differ).
    #[test]
    fn segment_fields_round_trip() {
        let store = Store::open_in_memory().expect("open");
        let segment = &fixture_segments()[0];
        store
            .insert(Utc::now(), std::slice::from_ref(segment))
            .expect("insert");

        let (trip_id, mode, realtime, departure, scheduled_departure, polyline, from_lat): (
            String,
            String,
            bool,
            i64,
            i64,
            String,
            f64,
        ) = store
            .conn
            .query_row(
                "SELECT trip_id, mode, realtime, departure, scheduled_departure, polyline, from_lat
                 FROM segment",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .expect("row");

        assert_eq!(trip_id, segment.trips[0].trip_id);
        assert_eq!(mode, segment.mode.to_string());
        assert_eq!(realtime, segment.real_time);
        assert_eq!(departure, segment.departure.timestamp_millis());
        assert_eq!(scheduled_departure, segment.scheduled_departure.timestamp_millis());
        assert_ne!(departure, scheduled_departure, "fixture rail leg carries a delay");
        assert_eq!(polyline, segment.polyline);
        assert_eq!(from_lat, segment.from.lat);
    }
}
