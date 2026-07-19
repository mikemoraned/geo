//! Ingest the raw `motis` capture log into the derived `train_segment` table of the
//! `lookout` db: dedup the duplication-allowed rows down to one per scheduled leg
//! (newest capture wins, so its realtime-corrected times survive), decode each Google
//! polyline to a lat/lon line, and store it as WKB — the same `(lon, lat)` WKB the
//! Python visualiser reads from the `transport` table. Idempotent `INSERT OR IGNORE`,
//! mirroring `enrich`, so re-running doesn't duplicate rows.

use rusqlite::{params, Connection};
use wkb::writer::{write_line_string, WriteOptions};

/// Precision the Motis `map/trips` polylines are encoded at.
const POLYLINE_PRECISION: u32 = 5;

/// DDL for the derived table; `IF NOT EXISTS` leaves the `lookout` db's other tables
/// (`gps`, `transport`, …) untouched. The unique key is the scheduled-leg identity
/// `(trip_id, from_stop_id, departure)` — the same one dedup collapses on — so re-ingest
/// is an idempotent no-op. `departure` alone is not unique per trip: minute-resolution
/// timetables let two legs of one trip depart different stops in the same minute.
const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS train_segment (
  rowid        INTEGER PRIMARY KEY,
  trip_id      TEXT NOT NULL,
  route_name   TEXT,
  mode         TEXT NOT NULL,
  route_color  TEXT,
  realtime     INTEGER NOT NULL,
  from_stop_id TEXT,
  departure    INTEGER NOT NULL,
  arrival      INTEGER NOT NULL,
  geom         BLOB NOT NULL,
  UNIQUE(trip_id, from_stop_id, departure)
);
";

/// What one ingest run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestOutcome {
    /// Distinct legs after dedup.
    pub deduped: usize,
    /// Rows newly written (existing `(trip_id, departure)` are ignored).
    pub written: usize,
}

/// A failure ingesting the capture log.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("decoding polyline: {0}")]
    Polyline(String),
    #[error("encoding wkb: {0}")]
    Wkb(#[from] wkb::error::WkbError),
}

/// One deduped raw segment: the kept fields plus the still-encoded polyline.
struct RawSegment {
    trip_id: String,
    route_name: Option<String>,
    mode: String,
    route_color: Option<String>,
    realtime: bool,
    from_stop_id: Option<String>,
    departure: i64,
    arrival: i64,
    polyline: String,
}

/// A derived segment: the kept fields plus the decoded geometry as WKB.
struct TrainSegment {
    trip_id: String,
    route_name: Option<String>,
    mode: String,
    route_color: Option<String>,
    realtime: bool,
    from_stop_id: Option<String>,
    departure: i64,
    arrival: i64,
    geom: Vec<u8>,
}

/// Dedup + decode the raw `segment` log in `source` into `dest`'s `train_segment` table.
pub fn ingest(source: &Connection, dest: &Connection) -> Result<IngestOutcome, IngestError> {
    let raws = read_deduped(source)?;
    let deduped = raws.len();
    let segments = raws
        .into_iter()
        .map(RawSegment::into_train_segment)
        .collect::<Result<Vec<_>, _>>()?;
    let written = write_train_segments(dest, &segments)?;
    Ok(IngestOutcome { deduped, written })
}

/// Read the raw log, collapsing re-sightings of the same scheduled leg
/// (`trip_id, from_stop_id, departure`) to the newest `captured_at`'s row.
fn read_deduped(source: &Connection) -> Result<Vec<RawSegment>, IngestError> {
    let mut stmt = source.prepare(
        "SELECT trip_id, route_name, mode, route_color, realtime, from_stop_id,
                departure, arrival, polyline
         FROM (
           SELECT *, ROW_NUMBER() OVER (
             PARTITION BY trip_id, from_stop_id, departure ORDER BY captured_at DESC
           ) AS rn
           FROM segment
         )
         WHERE rn = 1",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(RawSegment {
            trip_id: r.get(0)?,
            route_name: r.get(1)?,
            mode: r.get(2)?,
            route_color: r.get(3)?,
            realtime: r.get(4)?,
            from_stop_id: r.get(5)?,
            departure: r.get(6)?,
            arrival: r.get(7)?,
            polyline: r.get(8)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(IngestError::from)
}

/// Create the table if needed and append the segments idempotently, returning the
/// number of rows newly inserted.
fn write_train_segments(
    dest: &Connection,
    segments: &[TrainSegment],
) -> Result<usize, IngestError> {
    dest.execute_batch(SCHEMA)?;
    let tx = dest.unchecked_transaction()?;
    let mut written = 0;
    for s in segments {
        written += tx.execute(
            "INSERT OR IGNORE INTO train_segment
               (trip_id, route_name, mode, route_color, realtime, from_stop_id,
                departure, arrival, geom)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                s.trip_id,
                s.route_name,
                s.mode,
                s.route_color,
                s.realtime,
                s.from_stop_id,
                s.departure,
                s.arrival,
                s.geom,
            ],
        )?;
    }
    tx.commit()?;
    Ok(written)
}

impl RawSegment {
    fn into_train_segment(self) -> Result<TrainSegment, IngestError> {
        Ok(TrainSegment {
            geom: polyline_to_wkb(&self.polyline)?,
            trip_id: self.trip_id,
            route_name: self.route_name,
            mode: self.mode,
            route_color: self.route_color,
            realtime: self.realtime,
            from_stop_id: self.from_stop_id,
            departure: self.departure,
            arrival: self.arrival,
        })
    }
}

/// Decode a Google-encoded polyline to a `(lon, lat)` WKB `LineString` (little-endian,
/// OGC — what `shapely.from_wkb` reads on the Python side).
fn polyline_to_wkb(encoded: &str) -> Result<Vec<u8>, IngestError> {
    let line = polyline::decode_polyline(encoded, POLYLINE_PRECISION)
        .map_err(|e| IngestError::Polyline(e.to_string()))?;
    let mut geom = Vec::new();
    write_line_string(&mut geom, &line, &WriteOptions::default())?;
    Ok(geom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use motis_openapi_progenitor::types::TripSegment;

    fn fixture() -> Vec<TripSegment> {
        serde_json::from_str(include_str!("../tests/fixtures/trips.json")).expect("parse fixture")
    }

    /// The raw log re-seen across polls collapses to one row per leg, and each polyline
    /// decodes into a WKB LineString with the same number of points.
    #[test]
    fn dedups_and_decodes_fixture_rows() {
        let source = crate::store::Store::open_in_memory().expect("source");
        let fixture = fixture();
        let t1 = Utc::now();
        source.insert(t1, &fixture).expect("insert t1");
        source
            .insert(t1 + chrono::Duration::seconds(30), &fixture)
            .expect("insert t2"); // same legs re-seen → duplicates

        let dest = Connection::open_in_memory().expect("dest");
        let outcome = ingest(source.connection(), &dest).expect("ingest");

        assert_eq!(outcome.deduped, fixture.len(), "8 raw rows collapse to 4 legs");
        assert_eq!(outcome.written, fixture.len());
        let count: i64 = dest
            .query_row("SELECT COUNT(*) FROM train_segment", [], |r| r.get(0))
            .expect("count");
        assert_eq!(count as usize, fixture.len());

        // The stored geom is a little-endian WKB LineString whose point count matches
        // the decoded polyline.
        let (trip_id, geom): (String, Vec<u8>) = dest
            .query_row("SELECT trip_id, geom FROM train_segment LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("row");
        let segment = fixture
            .iter()
            .find(|s| s.trips[0].trip_id == trip_id)
            .expect("matching fixture segment");
        let expected_points = polyline::decode_polyline(&segment.polyline, POLYLINE_PRECISION)
            .expect("decode")
            .0
            .len();

        assert_eq!(geom[0], 1, "little-endian byte order marker");
        assert_eq!(u32::from_le_bytes(geom[1..5].try_into().unwrap()), 2, "LineString type");
        assert_eq!(
            u32::from_le_bytes(geom[5..9].try_into().unwrap()) as usize,
            expected_points,
        );
    }

    /// Among re-sightings of one leg, the newest `captured_at` wins — so its
    /// realtime-corrected `arrival` and `realtime` flag are what land.
    #[test]
    fn dedup_prefers_newest_captured_at() {
        let source = crate::store::Store::open_in_memory().expect("source");
        let insert = "INSERT INTO segment
            (captured_at, trip_id, route_name, mode, route_color, from_stop_id,
             from_lat, from_lon, to_stop_id, to_lat, to_lon,
             departure, arrival, scheduled_departure, scheduled_arrival, realtime, polyline)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)";
        let poly = "_p~iF~ps|U_ulLnnqC_mqNvxq`@";
        let conn = source.connection();
        // Older capture: scheduled (realtime false), arrival 600.
        conn.execute(
            insert,
            params![1000i64, "trip-1", "RB1", "REGIONAL_RAIL", None::<String>, "stop-a",
                50.0, 8.0, "stop-b", 50.1, 8.1, 500i64, 600i64, 480i64, 580i64, false, poly],
        )
        .expect("older");
        // Newer capture of the same leg: realtime true, arrival delayed to 700.
        conn.execute(
            insert,
            params![2000i64, "trip-1", "RB1", "REGIONAL_RAIL", None::<String>, "stop-a",
                50.0, 8.0, "stop-b", 50.1, 8.1, 500i64, 700i64, 480i64, 580i64, true, poly],
        )
        .expect("newer");

        let dest = Connection::open_in_memory().expect("dest");
        let outcome = ingest(conn, &dest).expect("ingest");

        assert_eq!(outcome.deduped, 1);
        let (arrival, realtime): (i64, bool) = dest
            .query_row("SELECT arrival, realtime FROM train_segment", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("row");
        assert_eq!(arrival, 700, "newest capture's arrival");
        assert!(realtime, "newest capture's realtime flag");
    }
}
