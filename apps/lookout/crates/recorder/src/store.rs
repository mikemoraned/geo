//! The SQLite archive the recorder writes telemetry into.
//!
//! Three kinds of table:
//!   - `raw` — the lossless store: every queue payload, verbatim JSON, keyed on its
//!     md5 so re-recording the same payload is idempotent. A `received_at` column
//!     carries the queue item's server-stamped receive time (see [`RawSample`]); it
//!     sits beside the JSON rather than inside it, so the payload stays verbatim and
//!     the md5 contract is untouched.
//!   - per-sensor (`accel`, `gps`) — a derivation of `raw`, one row per reading,
//!     deduped on `(device_id, t)` so re-recording an already-seen reading is a no-op.
//!   - `device` — session metadata from a v1 `StartSession`, keyed on `device_id`,
//!     for the per-sensor tables to join to.
//!
//! Reading rows are idempotent (`INSERT OR IGNORE`) and a `StartSession` upserts the
//! device row, so both recorder modes (a non-destructive peek and a destructive
//! drain) can run against the same DB repeatedly without duplicating rows.

use std::path::Path;

use rusqlite::Connection;
use shared::{AccelReading, DeviceType, GpsReading, Message, SessionStart, V0Message, V1Message};
use telemetry::RawSample;

/// DDL run on open; `IF NOT EXISTS` makes opening an existing archive a no-op.
const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS raw (
  md5         TEXT NOT NULL PRIMARY KEY,
  json        TEXT NOT NULL,
  received_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS accel (
  device_id TEXT NOT NULL,
  t         INTEGER NOT NULL,
  rms REAL, peak REAL, n INTEGER,
  x REAL, y REAL, z REAL,
  PRIMARY KEY (device_id, t)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS gps (
  device_id TEXT NOT NULL,
  t         INTEGER NOT NULL,
  lat REAL, lon REAL, alt REAL, acc REAL, speed REAL, heading REAL,
  PRIMARY KEY (device_id, t)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS device (
  device_id   TEXT NOT NULL PRIMARY KEY,
  t           INTEGER NOT NULL,
  device_type TEXT,
  platform    TEXT,
  user_agent  TEXT,
  os          TEXT,
  os_version  TEXT
) WITHOUT ROWID;
";

/// Failure writing a sample to the archive.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("failed to parse sample json: {0}")]
    Parse(#[from] serde_json::Error),
}

/// A handle to the SQLite archive.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating if absent) the archive at `path` and ensure the schema exists.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Open an in-memory archive (its schema created); the data is discarded on drop.
    /// Test-only: production always archives to a file via [`Store::open`].
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Archive one queue payload: the lossless `raw` row plus the derived row its
    /// message carries. The raw JSON is stored verbatim; a payload that fails to parse
    /// is still archived losslessly, then surfaced as [`StoreError::Parse`].
    ///
    /// Messages route by version and type, but both protocol versions populate the
    /// same per-sensor tables, so a v0 payload still derives `accel` / `gps` rows.
    pub fn insert(&self, raw: &RawSample) -> Result<(), StoreError> {
        let json = raw.json();
        let md5 = format!("{:x}", md5::compute(json));
        self.conn.execute(
            "INSERT OR IGNORE INTO raw (md5, json, received_at) VALUES (?1, ?2, ?3)",
            (&md5, json, raw.received_at()),
        )?;

        match raw.parse()? {
            Message::Version0(V0Message::Gps(r)) => self.insert_gps(&r)?,
            Message::Version0(V0Message::Acceleration(r)) => self.insert_accel(&r)?,
            Message::Version1(V1Message::Gps(r)) => self.insert_gps(&r)?,
            Message::Version1(V1Message::Acceleration(r)) => self.insert_accel(&r)?,
            Message::Version1(V1Message::StartSession(s)) => self.insert_device(&s)?,
        }
        Ok(())
    }

    fn insert_gps(&self, r: &GpsReading) -> Result<(), rusqlite::Error> {
        let device_id = r.id.to_string();
        self.ensure_device(&device_id, r.t)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO gps (device_id, t, lat, lon, alt, acc, speed, heading)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (
                &device_id,
                r.t,
                r.gps.lat,
                r.gps.lon,
                r.gps.alt,
                r.gps.acc,
                r.gps.speed,
                r.gps.heading,
            ),
        )?;
        Ok(())
    }

    fn insert_accel(&self, r: &AccelReading) -> Result<(), rusqlite::Error> {
        let device_id = r.id.to_string();
        self.ensure_device(&device_id, r.t)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO accel (device_id, t, rms, peak, n, x, y, z)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (
                &device_id,
                r.t,
                r.accel.rms,
                r.accel.peak,
                r.accel.n,
                r.accel.x,
                r.accel.y,
                r.accel.z,
            ),
        )?;
        Ok(())
    }

    /// Seed a minimal `unknown` device row so every `device_id` in a per-sensor table
    /// has a row to join to, even for v0 payloads that carry no session metadata.
    /// `INSERT OR IGNORE` never clobbers a fuller row a `StartSession` writes, and a
    /// later `StartSession` upserts the full metadata over this placeholder.
    fn ensure_device(&self, device_id: &str, t: i64) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT OR IGNORE INTO device (device_id, t, device_type) VALUES (?1, ?2, ?3)",
            (device_id, t, DeviceType::Unknown.as_str()),
        )?;
        Ok(())
    }

    /// Upsert a device's session metadata; a later session refreshes it in place.
    fn insert_device(&self, s: &SessionStart) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO device (device_id, t, device_type, platform, user_agent, os, os_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(device_id) DO UPDATE SET
               t=excluded.t, device_type=excluded.device_type, platform=excluded.platform,
               user_agent=excluded.user_agent, os=excluded.os, os_version=excluded.os_version",
            (
                &s.id.to_string(),
                s.t,
                s.device.device_type.as_str(),
                &s.device.platform,
                &s.device.user_agent,
                &s.device.os,
                &s.device.os_version,
            ),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(store: &Store, table: &str) -> i64 {
        store
            .conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count")
    }

    const GPS_JSON: &str = r#"{"id":"00000000-0000-0000-0000-000000000001","t":1700000000000,"gps":{"lat":55.95,"lon":-3.19,"alt":80.0,"acc":5.0}}"#;
    const ACCEL_JSON: &str = r#"{"id":"00000000-0000-0000-0000-000000000001","t":1700000000000,"accel":{"x":0.1,"y":-9.8,"z":0.3}}"#;
    const V1_GPS_JSON: &str = r#"{"v":1,"type":"gps","id":"00000000-0000-0000-0000-000000000001","t":1700000000005,"gps":{"lat":55.95,"lon":-3.19,"alt":80.0,"acc":5.0,"speed":31.4,"heading":275.0}}"#;
    const V1_ACCEL_JSON: &str = r#"{"v":1,"type":"acceleration","id":"00000000-0000-0000-0000-000000000001","t":1700000000006,"accel":{"rms":0.42,"peak":1.7,"n":600,"x":0.1,"y":-9.8,"z":0.3}}"#;
    const SESSION_JSON: &str = r#"{"v":1,"type":"start_session","id":"00000000-0000-0000-0000-000000000001","t":1700000000000,"device":{"device_type":"iphone","platform":"iPhone","user_agent":"Safari","os":"iOS","os_version":"18.5"}}"#;

    /// The server-stamped receive time the queue item would carry.
    const RECEIVED_AT: i64 = 1_700_000_050_000;

    /// Wrap a payload as the recorder would see it off the queue.
    fn raw(json: &str) -> RawSample {
        RawSample::new(RECEIVED_AT, json)
    }

    #[test]
    fn insert_populates_raw_and_matching_sensor_table() {
        let store = Store::open_in_memory().expect("open");
        store.insert(&raw(GPS_JSON)).expect("insert gps");
        store
            .insert(&raw(ACCEL_JSON))
            .expect("insert accel");

        assert_eq!(count(&store, "raw"), 2);
        assert_eq!(count(&store, "gps"), 1);
        assert_eq!(count(&store, "accel"), 1);
    }

    /// A v1 sensor payload derives the same per-sensor rows as its v0 shape.
    #[test]
    fn v1_sensor_payload_populates_sensor_table() {
        let store = Store::open_in_memory().expect("open");
        store
            .insert(&raw(V1_GPS_JSON))
            .expect("insert v1 gps");

        assert_eq!(count(&store, "gps"), 1);
    }

    /// The data-quality columns land in the per-sensor tables.
    #[test]
    fn sensor_rows_store_new_columns() {
        let store = Store::open_in_memory().expect("open");
        store.insert(&raw(V1_GPS_JSON)).expect("gps");
        store.insert(&raw(V1_ACCEL_JSON)).expect("accel");

        let (speed, heading): (Option<f64>, Option<f64>) = store
            .conn
            .query_row("SELECT speed, heading FROM gps", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .expect("gps row");
        assert_eq!(speed, Some(31.4));
        assert_eq!(heading, Some(275.0));

        let (rms, peak, n): (f64, f64, i64) = store
            .conn
            .query_row("SELECT rms, peak, n FROM accel", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .expect("accel row");
        assert_eq!(rms, 0.42);
        assert_eq!(peak, 1.7);
        assert_eq!(n, 600);
    }

    /// The raw row stores the queue item's server-stamped `received_at` verbatim,
    /// distinct from the device-stamped `t` inside the payload.
    #[test]
    fn raw_row_is_stamped_with_received_at() {
        let store = Store::open_in_memory().expect("open");
        store.insert(&raw(GPS_JSON)).expect("insert gps");

        let received_at: i64 = store
            .conn
            .query_row("SELECT received_at FROM raw", [], |row| row.get(0))
            .expect("received_at");
        assert_eq!(received_at, RECEIVED_AT);
    }

    /// Every device_id in a per-sensor table has a device row to join to, even for a
    /// v0 payload with no session metadata: a minimal `unknown` placeholder is seeded.
    #[test]
    fn sensor_reading_seeds_a_minimal_device_row() {
        let store = Store::open_in_memory().expect("open");
        store.insert(&raw(GPS_JSON)).expect("insert gps");

        assert_eq!(count(&store, "device"), 1);
        let (device_type, platform): (String, Option<String>) = store
            .conn
            .query_row("SELECT device_type, platform FROM device", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .expect("device row");
        assert_eq!(device_type, "unknown");
        assert_eq!(platform, None, "a v0 reading carries no platform metadata");
    }

    /// A StartSession arriving after a placeholder upgrades it to full metadata in
    /// place, and a later sensor reading doesn't clobber it back to `unknown`.
    #[test]
    fn start_session_upgrades_placeholder_and_survives_later_readings() {
        let store = Store::open_in_memory().expect("open");
        store.insert(&raw(GPS_JSON)).expect("v0 reading");
        store
            .insert(&raw(SESSION_JSON))
            .expect("start session");
        store
            .insert(&raw(ACCEL_JSON))
            .expect("later reading");

        assert_eq!(count(&store, "device"), 1);
        let device_type: String = store
            .conn
            .query_row("SELECT device_type FROM device", [], |row| row.get(0))
            .expect("device_type");
        assert_eq!(device_type, "iphone", "session metadata is not clobbered");
    }

    /// A v1 StartSession upserts the device row (and touches no sensor table).
    #[test]
    fn start_session_populates_device_table() {
        let store = Store::open_in_memory().expect("open");
        store
            .insert(&raw(SESSION_JSON))
            .expect("insert session");

        assert_eq!(count(&store, "raw"), 1);
        assert_eq!(count(&store, "device"), 1);
        assert_eq!(count(&store, "gps"), 0);
        assert_eq!(count(&store, "accel"), 0);

        let platform: String = store
            .conn
            .query_row("SELECT platform FROM device", [], |row| row.get(0))
            .expect("platform");
        assert_eq!(platform, "iPhone");
    }

    /// A second session for the same device refreshes the row in place (upsert).
    #[test]
    fn start_session_upserts_on_device_id() {
        let laptop = r#"{"v":1,"type":"start_session","id":"00000000-0000-0000-0000-000000000001","t":1700000000010,"device":{"device_type":"laptop","platform":"MacIntel","user_agent":"Safari","os":"macOS","os_version":"15.0"}}"#;
        let store = Store::open_in_memory().expect("open");
        store.insert(&raw(SESSION_JSON)).expect("first");
        store.insert(&raw(laptop)).expect("second");

        assert_eq!(count(&store, "device"), 1);
        let device_type: String = store
            .conn
            .query_row("SELECT device_type FROM device", [], |row| row.get(0))
            .expect("device_type");
        assert_eq!(device_type, "laptop", "later session wins on upsert");
    }

    #[test]
    fn reinserting_identical_payload_is_idempotent() {
        let store = Store::open_in_memory().expect("open");
        store.insert(&raw(GPS_JSON)).expect("first");
        store.insert(&raw(GPS_JSON)).expect("second");

        assert_eq!(count(&store, "raw"), 1);
        assert_eq!(count(&store, "gps"), 1);
    }

    /// Two distinct payloads for the same `(device_id, t)` are both lossless-archived
    /// (different md5), but the derived sensor table keeps only the first.
    #[test]
    fn same_key_different_payload_dedupes_sensor_but_not_raw() {
        let other = r#"{"id":"00000000-0000-0000-0000-000000000001","t":1700000000000,"gps":{"lat":55.96,"lon":-3.19,"alt":80.0,"acc":5.0}}"#;
        let store = Store::open_in_memory().expect("open");
        store.insert(&raw(GPS_JSON)).expect("first");
        store.insert(&raw(other)).expect("second");

        assert_eq!(count(&store, "raw"), 2);
        assert_eq!(count(&store, "gps"), 1);
        let lat: f64 = store
            .conn
            .query_row("SELECT lat FROM gps", [], |row| row.get(0))
            .expect("lat");
        assert_eq!(lat, 55.95, "first payload wins on dedup");
    }
}
