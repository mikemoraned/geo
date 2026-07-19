//! Read side of the lookout SQLite archive: derives the per-`(device_id, UTC day)`
//! bounding boxes from its `gps` fixes, in SQL. The archive is written by the
//! `recorder` crate; here it is opened read-only, so this crate owns none of the
//! schema.

use std::path::Path;

use rusqlite::Connection;

use geo_types::{Coord, Rect};

use crate::groups::{Group, GroupKey};

/// Milliseconds per day, for flooring an epoch-ms `t` to its UTC day in SQL.
const MS_PER_DAY: i64 = 86_400_000;

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

    /// Group the `gps` fixes by `(device_id, UTC day)` and reduce each group to its
    /// bounding box, entirely in SQL: SQLite floors the epoch-ms `t` to a day with
    /// integer division and aggregates the lat/lon extent with `MIN`/`MAX`. Ordered
    /// by key so the result is deterministic. (`t` is non-negative for real fixes, so
    /// the division truncates the same way a floor would.)
    pub fn groups(&self) -> Result<Vec<Group>, ArchiveError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT device_id, t / {MS_PER_DAY} AS day,
                    MIN(lat) AS min_lat, MAX(lat) AS max_lat,
                    MIN(lon) AS min_lon, MAX(lon) AS max_lon
             FROM gps
             GROUP BY device_id, day
             ORDER BY device_id, day"
        ))?;
        let groups = stmt
            .query_map([], |r| {
                Ok(Group {
                    key: GroupKey {
                        device_id: r.get("device_id")?,
                        day: r.get("day")?,
                    },
                    bbox: Rect::new(
                        Coord {
                            x: r.get("min_lon")?,
                            y: r.get("min_lat")?,
                        },
                        Coord {
                            x: r.get("max_lon")?,
                            y: r.get("max_lat")?,
                        },
                    ),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(groups)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lat/lon [`Rect`] from the extent, keeping tests in `(lat, lon)` reading order.
    fn rect(min_lat: f64, max_lat: f64, min_lon: f64, max_lon: f64) -> Rect<f64> {
        Rect::new(
            Coord {
                x: min_lon,
                y: min_lat,
            },
            Coord {
                x: max_lon,
                y: max_lat,
            },
        )
    }

    /// Mirrors the recorder's real `gps` table (`crates/recorder/src/store.rs`); this
    /// crate only *reads* from it, and `groups` aggregates just `device_id`/`t`/`lat`/
    /// `lon`, but the fixture keeps the full column set so it's clear what it reads.
    const GPS_SCHEMA: &str = "\
CREATE TABLE gps (
  device_id TEXT NOT NULL, t INTEGER NOT NULL,
  lat REAL, lon REAL, alt REAL, acc REAL, speed REAL, heading REAL,
  PRIMARY KEY (device_id, t)
) WITHOUT ROWID;";

    fn archive_with_rows(rows: &[(&str, i64, f64, f64)]) -> Archive {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(GPS_SCHEMA).expect("schema");
        for (device_id, t, lat, lon) in rows {
            conn.execute(
                "INSERT INTO gps (device_id, t, lat, lon) VALUES (?1, ?2, ?3, ?4)",
                (device_id, t, lat, lon),
            )
            .expect("insert");
        }
        Archive { conn }
    }

    /// Captured-shape fixes: two for one device on the same UTC day (the first is late
    /// enough in the day that the third, 1h46m later, crosses midnight), one the next.
    const CAPTURED: &[(&str, i64, f64, f64)] = &[
        ("dev-a", 1_700_000_000_000, 55.95, -3.19),
        ("dev-a", 1_700_000_060_000, 55.97, -3.15),
        ("dev-a", 1_700_006_400_000, 56.00, -3.10),
    ];

    /// The two same-day fixes collapse to one group whose bbox spans their extent; the
    /// next-day fix is its own group.
    #[test]
    fn groups_fixes_into_per_day_bboxes() {
        let archive = archive_with_rows(CAPTURED);
        let groups = archive.groups().expect("groups");

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].bbox, rect(55.95, 55.97, -3.19, -3.15));
        assert_eq!(groups[0].key.day + 1, groups[1].key.day);
        assert_eq!(groups[1].bbox, rect(56.00, 56.00, -3.10, -3.10));
    }

    /// A single fix yields a degenerate (point) box.
    #[test]
    fn single_fix_yields_a_point_box() {
        let archive = archive_with_rows(&[("dev-a", 1_700_000_000_000, 55.95, -3.19)]);
        let groups = archive.groups().expect("groups");

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].bbox, rect(55.95, 55.95, -3.19, -3.19));
    }

    /// Different devices on the same day are distinct groups, returned sorted by key.
    #[test]
    fn different_devices_are_separate_groups_sorted_by_key() {
        let archive = archive_with_rows(&[
            ("dev-b", 1_700_000_000_000, 51.50, -0.12),
            ("dev-a", 1_700_000_000_000, 55.95, -3.19),
        ]);
        let groups = archive.groups().expect("groups");

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key.device_id, "dev-a");
        assert_eq!(groups[1].key.device_id, "dev-b");
    }

    #[test]
    fn no_rows_yields_no_groups() {
        let archive = archive_with_rows(&[]);
        assert!(archive.groups().expect("groups").is_empty());
    }
}
