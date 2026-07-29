//! Deriving sessions twice: what the store holds after a rerun.
//!
//! Sessionisation re-derives every session from all of bronze on every run, and the newest
//! session is always still open — the next drain adds to it. The two things that has to
//! mean are checked here end to end, through the same archive the drain writes with: a
//! rerun over unchanged bronze leaves the store as it was, and a rerun over bronze that has
//! grown adds the new samples to the session they belong to rather than starting another.

use chrono::{DateTime, Duration, TimeZone, Utc};
use medallion::{Country, Query, Root};
use model::{DeviceId, SessionId};
use recorder::bronze::{Archive, Payload};
use recorder::sessions::{sessions, Gap};
use recorder::silver;
use serde::Deserialize;
use shared::{Gps, GpsReading, Message, V1Message};
use uuid::Uuid;

/// One session as the store holds it.
#[derive(Debug, Deserialize, PartialEq)]
struct Session {
    session_id: SessionId,
    device_id: DeviceId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    started_at: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    ended_at: DateTime<Utc>,
    sample_count: u32,
}

fn at(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 26, 9, minute, 0).unwrap()
}

fn gps(id: Uuid, t: DateTime<Utc>, lat: f64) -> Message {
    Message::Version1(V1Message::Gps(GpsReading {
        id,
        t: t.timestamp_millis(),
        gps: Gps {
            lat,
            lon: 13.4,
            alt: Some(38.0),
            acc: 5.0,
            speed: Some(27.0),
            heading: Some(91.0),
        },
    }))
}

/// Drain `messages` into bronze as one batch, the way the recorder does.
async fn drain(root: &Root, at: DateTime<Utc>, messages: &[Message]) {
    let json: Vec<String> = messages
        .iter()
        .map(|message| serde_json::to_string(message).expect("serialize"))
        .collect();
    let payloads: Vec<Payload> = json
        .iter()
        .map(|json| Payload {
            received_at: Some(at.timestamp_millis()),
            json,
        })
        .collect();
    Archive::new(root.clone())
        .write(at, &payloads)
        .await
        .expect("archive");
}

/// Derive every session in the store and write both silver datasets.
async fn sessionise(root: &Root) -> silver::WriteOutcome {
    let derived = sessions(root, Gap::default())
        .await
        .expect("derive sessions");
    silver::write(root, &derived, Country::Germany)
        .await
        .expect("write sessions")
}

async fn stored_sessions(root: &Root) -> Vec<Session> {
    let query = Query::new(root.clone());
    query
        .register(model::SESSION, "sessions")
        .await
        .expect("register");
    query
        .rows(
            "SELECT session_id, device_id, started_at, ended_at, sample_count
             FROM sessions ORDER BY started_at",
        )
        .await
        .expect("query sessions")
}

/// Every file under the store's silver layer, by path and size.
fn partitions(root: &Root) -> Vec<(String, u64)> {
    fn walk(dir: &std::path::Path, into: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(&path, into);
            } else {
                into.push(path);
            }
        }
    }

    let mut paths = Vec::new();
    walk(&root.path().join("silver"), &mut paths);
    paths.sort();
    paths
        .iter()
        .map(|path| {
            (
                path.strip_prefix(root.path())
                    .expect("under the root")
                    .display()
                    .to_string(),
                std::fs::metadata(path).expect("metadata").len(),
            )
        })
        .collect()
}

/// Every row of both silver datasets, rendered as text — the whole of what a reader gets
/// back, geometry included.
///
/// The files are compared through a reader rather than byte for byte because GeoParquet's
/// file metadata lists a dataset's geometry columns as a map, which serialises in a
/// different order from one write to the next. Nothing about the data varies with it.
async fn contents(root: &Root) -> String {
    let query = Query::new(root.clone());
    query
        .register(model::SESSION, "sessions")
        .await
        .expect("register sessions");
    query
        .register(model::SESSION_SAMPLE, "samples")
        .await
        .expect("register samples");

    let mut rendered = String::new();
    for sql in [
        "SELECT * FROM sessions ORDER BY session_id",
        "SELECT * FROM samples ORDER BY session_id, seq",
    ] {
        let batches = query.sql(sql).await.expect("query");
        rendered.push_str(
            &arrow::util::pretty::pretty_format_batches(&batches)
                .expect("render")
                .to_string(),
        );
    }
    rendered
}

/// A rerun over bronze nothing has been added to leaves the same partitions holding the
/// same rows: the derivation depends on what bronze holds and on nothing else about the run.
#[tokio::test]
async fn a_rerun_over_unchanged_bronze_produces_identical_partitions() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = Root::new(tmp.path());
    let device = Uuid::from_u128(1);
    drain(
        &root,
        at(0),
        &[
            gps(device, at(0), 52.5),
            gps(device, at(1), 52.6),
            gps(device, at(30), 52.7),
        ],
    )
    .await;

    let first = sessionise(&root).await;
    let after_first = (partitions(&root), contents(&root).await);
    let second = sessionise(&root).await;

    assert_eq!(first, second, "the same run, run twice");
    assert_eq!(
        (partitions(&root), contents(&root).await),
        after_first,
        "a rerun should leave every partition as it was"
    );
}

/// The newest session is open: samples that arrive later and follow it within the
/// threshold belong to it, and it keeps the id already written against it.
#[tokio::test]
async fn a_rerun_over_grown_bronze_extends_the_open_session() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = Root::new(tmp.path());
    let device = Uuid::from_u128(1);

    drain(
        &root,
        at(0),
        &[gps(device, at(0), 52.5), gps(device, at(1), 52.6)],
    )
    .await;
    sessionise(&root).await;
    let before = stored_sessions(&root).await;

    drain(&root, at(2), &[gps(device, at(2), 52.7)]).await;
    let outcome = sessionise(&root).await;

    assert_eq!(outcome.sessions, 1, "the samples extend one session");
    let after = stored_sessions(&root).await;
    assert_eq!(after.len(), 1);
    assert_eq!(
        after[0].session_id, before[0].session_id,
        "the session keeps the id already written against it"
    );
    assert_eq!(after[0].started_at, before[0].started_at);
    assert_eq!(
        after[0].ended_at,
        at(2),
        "the session now runs to the last sample"
    );
    assert_eq!(after[0].sample_count, 3);
}

/// A drain that repeats a sample bronze already holds — the queue re-sends an un-acked
/// tail — must not lengthen the session it belongs to.
#[tokio::test]
async fn a_rerun_over_bronze_that_repeats_a_sample_changes_nothing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = Root::new(tmp.path());
    let device = Uuid::from_u128(1);
    let samples = [gps(device, at(0), 52.5), gps(device, at(1), 52.6)];

    drain(&root, at(0), &samples).await;
    sessionise(&root).await;
    let before = stored_sessions(&root).await;

    drain(&root, at(1), &samples).await;
    let outcome = sessionise(&root).await;

    assert_eq!(
        outcome.samples, 2,
        "the repeat is deduped, not counted twice"
    );
    assert_eq!(stored_sessions(&root).await, before);
}

/// Samples arriving after a silence longer than the threshold are a second session, not a
/// continuation — the same evidence a single run would have split on.
#[tokio::test]
async fn a_rerun_after_a_long_silence_starts_a_second_session() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = Root::new(tmp.path());
    let device = Uuid::from_u128(1);

    drain(&root, at(0), &[gps(device, at(0), 52.5)]).await;
    sessionise(&root).await;
    let before = stored_sessions(&root).await;

    drain(
        &root,
        at(0) + Duration::hours(1),
        &[gps(device, at(0) + Duration::hours(1), 52.9)],
    )
    .await;
    sessionise(&root).await;

    let after = stored_sessions(&root).await;
    assert_eq!(after.len(), 2);
    assert_eq!(
        after[0].session_id, before[0].session_id,
        "the earlier session is untouched by a later one"
    );
    assert_ne!(after[1].session_id, before[0].session_id);
}
