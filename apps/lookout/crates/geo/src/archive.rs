//! Read side of the lookout SQLite archive: pulls the `gps` fixes the bbox grouping
//! is built from. The archive is written by the `recorder` crate; here it is opened
//! read-only, so this crate owns none of the schema.

use std::path::Path;

use rusqlite::Connection;

use crate::groups::GpsRow;

/// Failure opening or reading the archive.
#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// A read handle to a lookout SQLite archive.
pub struct Archive {
    conn: Connection,
}

impl Archive {
    /// Open the archive at `path`. Expects a `gps` table (created by the recorder);
    /// reading from a DB without one surfaces as [`ArchiveError::Sqlite`].
    pub fn open(path: &Path) -> Result<Self, ArchiveError> {
        Ok(Self {
            conn: Connection::open(path)?,
        })
    }

    /// Read every fix from the `gps` table — only the columns bbox grouping needs —
    /// ordered by device then time for a stable, readable stream.
    pub fn gps_rows(&self) -> Result<Vec<GpsRow>, ArchiveError> {
        let mut stmt = self
            .conn
            .prepare("SELECT device_id, t, lat, lon FROM gps ORDER BY device_id, t")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(GpsRow {
                    device_id: r.get(0)?,
                    t: r.get(1)?,
                    lat: r.get(2)?,
                    lon: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::groups::group_bboxes;

    /// The subset of the recorder's `gps` schema this crate reads. Kept minimal on
    /// purpose: enough columns to insert captured rows and read the four back.
    const GPS_SCHEMA: &str = "\
CREATE TABLE gps (
  device_id TEXT NOT NULL, t INTEGER NOT NULL,
  lat REAL, lon REAL, alt REAL, acc REAL, speed REAL, heading REAL,
  PRIMARY KEY (device_id, t)
) WITHOUT ROWID;";

    /// Captured-shape fixes: two for one device on the same UTC day, one the next.
    const CAPTURED: &[(&str, i64, f64, f64)] = &[
        ("dev-a", 1_700_000_000_000, 55.95, -3.19),
        ("dev-a", 1_700_000_060_000, 55.97, -3.15),
        ("dev-a", 1_700_006_400_000, 56.00, -3.10),
    ];

    fn archive_with_captured_rows() -> Archive {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(GPS_SCHEMA).expect("schema");
        for (device_id, t, lat, lon) in CAPTURED {
            conn.execute(
                "INSERT INTO gps (device_id, t, lat, lon) VALUES (?1, ?2, ?3, ?4)",
                (device_id, t, lat, lon),
            )
            .expect("insert");
        }
        Archive { conn }
    }

    #[test]
    fn gps_rows_reads_the_captured_fixes() {
        let archive = archive_with_captured_rows();
        let rows = archive.gps_rows().expect("read");
        assert_eq!(rows.len(), CAPTURED.len());
        assert_eq!(rows[0].device_id, "dev-a");
        assert_eq!(rows[0].lat, 55.95);
    }

    /// The read feeds grouping end-to-end: the captured rows collapse to two
    /// per-day boxes for the one device.
    #[test]
    fn captured_rows_group_into_per_day_bboxes() {
        let archive = archive_with_captured_rows();
        let groups = group_bboxes(archive.gps_rows().expect("read"));

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].bbox.min_lat, 55.95);
        assert_eq!(groups[0].bbox.max_lat, 55.97);
        assert_eq!(groups[1].bbox.min_lat, 56.00);
    }
}
