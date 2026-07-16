//! The SQLite archive the recorder writes telemetry into.
//!
//! Two kinds of table:
//!   - `raw` — the lossless store: every queue payload, verbatim JSON, keyed on its
//!     md5 so re-recording the same payload is idempotent.
//!   - per-sensor (`accel`, `gps`) — a derivation of `raw`, one row per reading,
//!     deduped on `(device_id, t)` so re-recording an already-seen reading is a no-op.
//!
//! Every write is idempotent (`INSERT OR IGNORE`), so both recorder modes (a
//! non-destructive peek and a destructive drain) can run against the same DB
//! repeatedly without duplicating rows.

use std::path::Path;

use rusqlite::Connection;
use telemetry::RawSample;

/// DDL run on open; `IF NOT EXISTS` makes opening an existing archive a no-op.
const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS raw (
  md5  TEXT NOT NULL PRIMARY KEY,
  json TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS accel (
  device_id TEXT NOT NULL,
  t         INTEGER NOT NULL,
  x REAL, y REAL, z REAL,
  PRIMARY KEY (device_id, t)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS gps (
  device_id TEXT NOT NULL,
  t         INTEGER NOT NULL,
  lat REAL, lon REAL, alt REAL, acc REAL,
  PRIMARY KEY (device_id, t)
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

    /// Archive one queue payload: the lossless `raw` row plus any per-sensor rows it
    /// carries. The raw JSON is stored verbatim; a payload that fails to parse is
    /// still archived losslessly, then surfaced as [`StoreError::Parse`].
    pub fn insert(&self, raw: &RawSample) -> Result<(), StoreError> {
        let json = raw.json();
        let md5 = format!("{:x}", md5::compute(json));
        self.conn.execute(
            "INSERT OR IGNORE INTO raw (md5, json) VALUES (?1, ?2)",
            (&md5, json),
        )?;

        let sample = raw.parse()?;
        let device_id = sample.id.to_string();
        if let Some(accel) = &sample.accel {
            self.conn.execute(
                "INSERT OR IGNORE INTO accel (device_id, t, x, y, z) VALUES (?1, ?2, ?3, ?4, ?5)",
                (&device_id, sample.t, accel.x, accel.y, accel.z),
            )?;
        }
        if let Some(gps) = &sample.gps {
            self.conn.execute(
                "INSERT OR IGNORE INTO gps (device_id, t, lat, lon, alt, acc) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                (&device_id, sample.t, gps.lat, gps.lon, gps.alt, gps.acc),
            )?;
        }
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

    #[test]
    fn insert_populates_raw_and_matching_sensor_table() {
        let store = Store::open_in_memory().expect("open");
        store.insert(&RawSample::new(GPS_JSON)).expect("insert gps");
        store
            .insert(&RawSample::new(ACCEL_JSON))
            .expect("insert accel");

        assert_eq!(count(&store, "raw"), 2);
        assert_eq!(count(&store, "gps"), 1);
        assert_eq!(count(&store, "accel"), 1);
    }

    #[test]
    fn reinserting_identical_payload_is_idempotent() {
        let store = Store::open_in_memory().expect("open");
        store.insert(&RawSample::new(GPS_JSON)).expect("first");
        store.insert(&RawSample::new(GPS_JSON)).expect("second");

        assert_eq!(count(&store, "raw"), 1);
        assert_eq!(count(&store, "gps"), 1);
    }

    /// Two distinct payloads for the same `(device_id, t)` are both lossless-archived
    /// (different md5), but the derived sensor table keeps only the first.
    #[test]
    fn same_key_different_payload_dedupes_sensor_but_not_raw() {
        let other = r#"{"id":"00000000-0000-0000-0000-000000000001","t":1700000000000,"gps":{"lat":55.96,"lon":-3.19,"alt":80.0,"acc":5.0}}"#;
        let store = Store::open_in_memory().expect("open");
        store.insert(&RawSample::new(GPS_JSON)).expect("first");
        store.insert(&RawSample::new(other)).expect("second");

        assert_eq!(count(&store, "raw"), 2);
        assert_eq!(count(&store, "gps"), 1);
        let lat: f64 = store
            .conn
            .query_row("SELECT lat FROM gps", [], |row| row.get(0))
            .expect("lat");
        assert_eq!(lat, 55.95, "first payload wins on dedup");
    }
}
