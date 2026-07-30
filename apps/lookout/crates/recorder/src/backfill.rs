//! Reading the pre-medallion sqlite archive into the bronze telemetry datasets.
//!
//! The old archive holds the payloads it drained in a `raw(md5, json, received_at)` table,
//! alongside the per-sensor tables it interpreted them into. Only `raw` is read here: the
//! readings are re-interpreted from it through the same [`Archive`](crate::bronze::Archive)
//! the live drain writes, so backfilled history is shaped by the current interpretation
//! rather than by whatever produced those older tables. `raw` being the lossless record is
//! what makes that possible.
//!
//! `received_at` is absent for payloads archived before receipt times were recorded, and is
//! carried through as absent.

use std::path::Path;

use chrono::{DateTime, Utc};
use medallion::{Query, Root};

use crate::bronze::{Archive, ArchiveError, Payload, Written};

/// The archived payloads, oldest first. Ordering is by receipt time so the file's rows read
/// as history does; the undated payloads sort first, being the oldest.
const PAYLOADS: &str = "SELECT md5, json, received_at FROM raw ORDER BY received_at, md5";

/// A failure backfilling.
#[derive(Debug, thiserror::Error)]
pub enum BackfillError {
    #[error("opening {path}: {source}")]
    Open {
        path: String,
        source: rusqlite::Error,
    },
    #[error("reading the archive: {0}")]
    Read(#[from] rusqlite::Error),
    #[error("payload {md5} does not hash to its key — the archive is not verbatim")]
    NotVerbatim { md5: String },
    #[error("the oldest payload ({md5}) is already in bronze — this archive has been backfilled")]
    AlreadyDone { md5: String },
    #[error("checking what bronze already holds: {0}")]
    Query(#[from] medallion::QueryError),
    #[error("writing the datasets: {0}")]
    Write(#[from] ArchiveError),
}

/// One payload as the old archive holds it.
struct Archived {
    md5: String,
    json: String,
    received_at: Option<i64>,
}

impl Archived {
    /// The payload as bronze takes it. The md5 is recomputed on the way in rather than
    /// copied, so a payload that no longer hashes to the key it was stored under is caught
    /// instead of being carried over under a key that does not describe it.
    fn payload(&self) -> Result<Payload<'_>, BackfillError> {
        let hash = format!("{:x}", md5::compute(&self.json));
        if hash != self.md5 {
            return Err(BackfillError::NotVerbatim {
                md5: self.md5.clone(),
            });
        }
        Ok(Payload {
            received_at: self.received_at,
            json: &self.json,
        })
    }
}

/// What a backfill wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Outcome {
    /// Payloads read out of the old archive.
    pub read: usize,
    /// What the datasets gained.
    pub written: Written,
    /// Payloads carried over with no receipt time, because none was recorded.
    pub undated: usize,
}

/// Read the archive at `db` into the bronze telemetry datasets under `root`, as one
/// ingestion at `ingested_at`.
///
/// The whole archive is written as a single ingestion — one file per dataset — so it either
/// lands or it does not: an archive is not large, and a half-written backfill would be worse
/// than none.
///
/// Refuses to run twice: bronze is immutable, so a second run would leave every payload in
/// it twice under two ingestions. The check is whether the archive's oldest payload is
/// already there — the oldest because it is the one least likely to have also arrived
/// through the queue around the changeover.
pub async fn backfill(
    db: &Path,
    root: &Root,
    ingested_at: DateTime<Utc>,
) -> Result<Outcome, BackfillError> {
    let archived = read(db)?;
    let Some(oldest) = archived.first() else {
        return Ok(Outcome::default());
    };
    if already_holds(root, &oldest.md5).await? {
        return Err(BackfillError::AlreadyDone {
            md5: oldest.md5.clone(),
        });
    }

    let payloads = archived
        .iter()
        .map(Archived::payload)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Outcome {
        read: archived.len(),
        undated: payloads
            .iter()
            .filter(|payload| payload.received_at.is_none())
            .count(),
        written: Archive::new(root.clone())
            .write(ingested_at, &payloads)
            .await?,
    })
}

fn read(db: &Path) -> Result<Vec<Archived>, BackfillError> {
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|source| BackfillError::Open {
                path: db.display().to_string(),
                source,
            })?;

    let mut statement = conn.prepare(PAYLOADS)?;
    let rows = statement.query_map([], |row| {
        Ok(Archived {
            md5: row.get(0)?,
            json: row.get(1)?,
            received_at: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Whether bronze already holds the payload keyed `md5`.
async fn already_holds(root: &Root, md5: &str) -> Result<bool, BackfillError> {
    let query = Query::new(root.clone());
    if !query
        .register_if_present(model::RAW_SAMPLE, "raw_sample")
        .await?
    {
        return Ok(false);
    }
    let count = query
        .count(&format!(
            "SELECT COUNT(*) AS count FROM raw_sample WHERE md5 = '{md5}'"
        ))
        .await?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use medallion::DatasetSpec;

    use super::*;

    /// The instant every test backfills at, so the files it writes are predictable.
    fn ingested_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 28, 9, 0, 0).unwrap()
    }

    /// A v1 gps payload, as the archive holds it.
    fn gps(t: i64) -> String {
        format!(
            "{{\"v\":1,\"type\":\"gps\",\"id\":\"00000000-0000-0000-0000-000000000001\",\
             \"t\":{t},\"gps\":{{\"lat\":50.1,\"lon\":8.6,\"alt\":null,\"acc\":5.0,\
             \"speed\":12.0,\"heading\":270.0}}}}"
        )
    }

    /// A v0 accel payload: no version, no type tag, and none of the aggregates.
    fn accel_v0(t: i64) -> String {
        format!(
            "{{\"id\":\"00000000-0000-0000-0000-000000000001\",\"t\":{t},\
             \"accel\":{{\"x\":0.1,\"y\":-9.8,\"z\":0.3}}}}"
        )
    }

    /// An archive holding `payloads` as `(json, received_at)`, keyed by md5 as the old
    /// recorder keyed it.
    fn archive_db(dir: &Path, payloads: &[(String, Option<i64>)]) -> std::path::PathBuf {
        let path = dir.join("lookout.sqlite");
        let conn = rusqlite::Connection::open(&path).expect("create archive");
        conn.execute(
            "CREATE TABLE raw (md5 TEXT NOT NULL PRIMARY KEY, json TEXT NOT NULL, received_at INTEGER)",
            [],
        )
        .expect("create raw");
        for (json, received_at) in payloads {
            conn.execute(
                "INSERT INTO raw (md5, json, received_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![format!("{:x}", md5::compute(json)), json, received_at],
            )
            .expect("insert payload");
        }
        path
    }

    async fn rows_in<L: medallion::LayerKind>(root: &Root, dataset: DatasetSpec<L>) -> i64 {
        let query = Query::new(root.clone());
        query.register(dataset, "d").await.expect("register");
        query
            .count("SELECT COUNT(*) AS count FROM d")
            .await
            .expect("count")
    }

    #[tokio::test]
    async fn every_archived_payload_lands_in_bronze_reinterpreted() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path().join("store"));
        let db = archive_db(
            tmp.path(),
            &[
                (gps(1_700_000_000_001), Some(1_700_000_050_000)),
                (gps(1_700_000_000_002), Some(1_700_000_051_000)),
                (accel_v0(1_600_000_000_000), None),
            ],
        );

        let outcome = backfill(&db, &root, ingested_at()).await.expect("backfill");

        assert_eq!(outcome.read, 3);
        assert_eq!(outcome.undated, 1);
        assert_eq!(outcome.written.raw, 3);
        assert_eq!(outcome.written.gps, 2);
        assert_eq!(outcome.written.accel, 1);
        assert_eq!(rows_in(&root, model::RAW_SAMPLE).await, 3);
        assert_eq!(rows_in(&root, model::GPS_READING).await, 2);
        assert_eq!(rows_in(&root, model::ACCEL_READING).await, 1);
    }

    /// The payloads land under the date they were backfilled on, not the date they were
    /// captured: `ingested_date` records when bronze gained them.
    #[tokio::test]
    async fn payloads_land_in_the_partition_of_the_run_not_of_their_capture() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path().join("store"));
        let db = archive_db(
            tmp.path(),
            &[(gps(1_700_000_000_001), Some(1_700_000_050_000))],
        );

        backfill(&db, &root, ingested_at()).await.expect("backfill");

        assert!(root
            .path()
            .join("bronze/gps_reading/ingested_date=2026-07-28")
            .is_dir());
    }

    /// Bronze is immutable, so a second run must not double every payload in it.
    #[tokio::test]
    async fn a_second_run_is_refused() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path().join("store"));
        let db = archive_db(
            tmp.path(),
            &[(gps(1_700_000_000_001), Some(1_700_000_050_000))],
        );
        backfill(&db, &root, ingested_at())
            .await
            .expect("first run");

        let err = backfill(&db, &root, ingested_at() + chrono::Duration::hours(1))
            .await
            .expect_err("second run");

        assert!(matches!(err, BackfillError::AlreadyDone { .. }), "{err:?}");
        assert_eq!(rows_in(&root, model::RAW_SAMPLE).await, 1);
    }

    /// A payload that no longer hashes to the key it is stored under is not the payload
    /// that was received, so it is reported rather than carried over.
    #[tokio::test]
    async fn a_payload_that_does_not_match_its_key_is_refused() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path().join("store"));
        let db = archive_db(
            tmp.path(),
            &[(gps(1_700_000_000_001), Some(1_700_000_050_000))],
        );
        let conn = rusqlite::Connection::open(&db).expect("open");
        conn.execute("UPDATE raw SET json = json || ' '", [])
            .expect("mangle payload");
        drop(conn);

        let err = backfill(&db, &root, ingested_at())
            .await
            .expect_err("mangled payload");

        assert!(matches!(err, BackfillError::NotVerbatim { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn an_empty_archive_writes_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path().join("store"));
        let db = archive_db(tmp.path(), &[]);

        let outcome = backfill(&db, &root, ingested_at()).await.expect("backfill");

        assert_eq!(outcome, Outcome::default());
        assert!(!root.path().join("bronze").exists());
    }
}
