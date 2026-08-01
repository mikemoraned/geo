//! Deriving the ground truth from what the store actually holds.
//!
//! The matching rule is checked in the unit tests; what cannot be checked there is the part
//! between them and the store — reading a session's envelope, its samples' positions in metres
//! and a crossing's, out of files the real writers produced. Those go through SQL and through
//! the projected geometry column, so they are exercised here against a store written by the
//! same code paths that write the real one.

use chrono::{DateTime, TimeZone, Utc};
use geo_types::Point;
use medallion::{
    geo_batch, projected_wkb_field, wkb_field, Countries, Country, Projector, Query, Root, COUNTRY,
    GEOMETRY, PROJECTED_GEOMETRY,
};
use model::{CrossingId, OverlapKind, WaterCrossingRow};
use recorder::bronze::{Archive, Payload};
use recorder::sessions::{sessions, Gap, Lead};
use recorder::silver;
use serde::Deserialize;
use shared::{Gps, GpsReading, Message, V1Message};
use uuid::Uuid;

use crossings::matching::Radius;

/// Every place in these tests is in Germany, which is where the coordinates are.
struct Germany;

impl Countries for Germany {
    fn containing(&self, _point: Point<f64>) -> Option<Country> {
        Some(Country::Germany)
    }
}

/// One pass as the store holds it.
#[derive(Debug, Deserialize, PartialEq)]
struct Pass {
    crossing_id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    crossed_at: DateTime<Utc>,
    distance_m: f64,
    samples_within: u32,
}

/// Berlin, where the samples in these tests run east from.
const LON: f64 = 13.404954;
const LAT: f64 = 52.520008;

/// Near enough to place a crossing a known number of metres from a sample; the distance
/// itself is measured in the projected column, not here.
fn east_of_berlin(metres: f64) -> f64 {
    LON + metres / 111_320.0 / f64::cos(LAT.to_radians())
}

fn at(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 22, 9, minute, 0).unwrap()
}

fn gps(id: Uuid, t: DateTime<Utc>, lon: f64) -> Message {
    Message::Version1(V1Message::Gps(GpsReading {
        id,
        t: t.timestamp_millis(),
        gps: Gps {
            lat: LAT,
            lon,
            alt: Some(38.0),
            acc: 5.0,
            speed: Some(27.0),
            heading: Some(91.0),
        },
    }))
}

/// A store holding one session running east from Berlin, sampled every minute.
async fn store_with_a_session(root: &Root, samples: usize) {
    let device = Uuid::new_v4();
    let messages: Vec<Message> = (0..samples)
        .map(|step| {
            gps(
                device,
                at(step as u32),
                east_of_berlin(step as f64 * 1_000.0),
            )
        })
        .collect();
    let json: Vec<String> = messages
        .iter()
        .map(|message| serde_json::to_string(message).expect("serialize"))
        .collect();
    let payloads: Vec<Payload> = json
        .iter()
        .map(|json| Payload {
            received_at: Some(at(0).timestamp_millis()),
            json,
        })
        .collect();

    Archive::new(root.clone())
        .write(at(0), &payloads)
        .await
        .expect("archive the samples");
    let derived = sessions(root, Gap::default(), Lead::default())
        .await
        .expect("derive the sessions");
    silver::write(root, &derived, &Germany)
        .await
        .expect("write the sessions");
}

/// Add crossings at the given distances east of Berlin, written the way the crossings
/// pipeline writes them: one file per country, both geometries, as GeoParquet.
async fn store_with_crossings(root: &Root, at_metres: &[f64]) {
    let projector = Projector::for_country(Country::Germany).expect("projector");
    let rows: Vec<WaterCrossingRow> = at_metres
        .iter()
        .enumerate()
        .map(|(n, _)| WaterCrossingRow {
            // The position is not among these columns: it is the geometry below.
            crossing_id: CrossingId::new(format!("water:track:rail@{n}")).expect("id"),
            water_id: "water".into(),
            water_subtype: Some("river".into()),
            water_class: Some("river".into()),
            track_id: "track".into(),
            rail_id: "rail".into(),
            rail_class: Some("rail".into()),
            overlap_kind: OverlapKind::Line,
            overlap_m: 40.0,
            total_overlap_m: 40.0,
            merged_parts: 1,
            frac: 0.5,
            extract_id: "20260727T193628Z".into(),
            merge_distance_m: 100.0,
            min_crossing_m: 5.0,
        })
        .collect();
    let points: Vec<Point<f64>> = at_metres
        .iter()
        .map(|metres| Point::new(east_of_berlin(*metres), LAT))
        .collect();
    let projected: Vec<Point<f64>> = points
        .iter()
        .map(|point| projector.project(point).expect("project"))
        .collect();

    let batch = geo_batch(
        &rows,
        &[
            (wkb_field(GEOMETRY).expect("field"), points.as_slice()),
            (
                projected_wkb_field(PROJECTED_GEOMETRY, Country::Germany).expect("field"),
                projected.as_slice(),
            ),
        ],
    )
    .expect("build the batch");

    root.rows_of::<WaterCrossingRow>()
        .partition(COUNTRY, Country::Germany)
        .expect("partition")
        .replace_with_geo(&[batch])
        .await
        .expect("write the crossings");
}

/// Every pass the store holds, oldest first.
async fn passes_in(root: &Root) -> Vec<Pass> {
    let query = Query::new(root.clone());
    if !query
        .register_if_present(model::SESSION_CROSSING, "session_crossing")
        .await
        .expect("register")
    {
        return Vec::new();
    }
    query
        .rows(
            "SELECT crossing_id, crossed_at, distance_m, samples_within
             FROM session_crossing ORDER BY crossed_at, crossing_id",
        )
        .await
        .expect("read the passes")
}

#[tokio::test]
async fn a_crossing_the_session_ran_past_is_recorded_against_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    // Samples at 0, 1000, 2000 m; a crossing 60 m past the second one.
    store_with_a_session(&root, 3).await;
    store_with_crossings(&root, &[1_060.0]).await;

    let outcome = crossings::silver::derive(&root, Radius::default())
        .await
        .expect("derive");

    assert_eq!(outcome.passes, 1);
    assert_eq!(outcome.sessions_matched, 1);
    let passes = passes_in(&root).await;
    assert_eq!(passes[0].crossed_at, at(1));
    assert!(
        (passes[0].distance_m - 60.0).abs() < 1.0,
        "measured {} m, which is not the metres it should be",
        passes[0].distance_m
    );
    assert_eq!(passes[0].samples_within, 1);
}

#[tokio::test]
async fn a_crossing_nowhere_near_the_session_is_not() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    store_with_a_session(&root, 3).await;
    store_with_crossings(&root, &[50_000.0]).await;

    let outcome = crossings::silver::derive(&root, Radius::default())
        .await
        .expect("derive");

    assert_eq!(outcome.passes, 0);
    assert_eq!(outcome.crossings, 1, "the crossing should still be read");
    assert!(passes_in(&root).await.is_empty());
}

/// The radius reaches the store, rather than being applied to something in metres that is
/// really degrees: 60 m is inside the default and outside a 20 m one.
#[tokio::test]
async fn the_radius_decides_what_counts_as_passed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    store_with_a_session(&root, 3).await;
    store_with_crossings(&root, &[1_060.0]).await;

    let narrow = crossings::silver::derive(&root, Radius::new(20.0))
        .await
        .expect("derive");

    assert_eq!(narrow.passes, 0);
    assert!(passes_in(&root).await.is_empty());
}

/// A rerun re-derives everything and replaces it, so what the store holds after two runs is
/// what it holds after one.
#[tokio::test]
async fn a_rerun_leaves_the_same_passes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    store_with_a_session(&root, 3).await;
    store_with_crossings(&root, &[1_060.0, 2_020.0]).await;

    crossings::silver::derive(&root, Radius::default())
        .await
        .expect("derive");
    let first = passes_in(&root).await;
    let second_run = crossings::silver::derive(&root, Radius::default())
        .await
        .expect("derive again");

    assert_eq!(second_run.passes, first.len());
    assert_eq!(passes_in(&root).await, first);
}

/// A crossing the run no longer matches leaves nothing behind: the dataset is what the last
/// run derived, not the union of every run.
#[tokio::test]
async fn a_pass_the_rerun_no_longer_makes_is_swept() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    store_with_a_session(&root, 3).await;
    store_with_crossings(&root, &[1_060.0]).await;
    crossings::silver::derive(&root, Radius::default())
        .await
        .expect("derive");

    let narrowed = crossings::silver::derive(&root, Radius::new(20.0))
        .await
        .expect("derive again");

    assert_eq!(narrowed.partitions.removed, 1);
    assert!(passes_in(&root).await.is_empty());
}

/// The dataset is laid out by the date the crossing was passed, so a session running over
/// midnight has its passes split the way its samples are.
#[tokio::test]
async fn passes_are_partitioned_by_the_date_they_happened_on() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    store_with_a_session(&root, 3).await;
    store_with_crossings(&root, &[1_060.0]).await;

    crossings::silver::derive(&root, Radius::default())
        .await
        .expect("derive");

    assert!(tmp
        .path()
        .join("silver/session_crossing/crossed_date=2026-07-22/part-0.parquet")
        .exists());
}

/// A session of one sample can still pass a crossing it sat beside, and says so with the one
/// sample it has — the count is what a reader weighs, and it is not filtered out here.
#[tokio::test]
async fn a_session_that_never_moved_still_records_what_it_sat_beside() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    store_with_a_session(&root, 1).await;
    store_with_crossings(&root, &[30.0]).await;

    let outcome = crossings::silver::derive(&root, Radius::default())
        .await
        .expect("derive");

    assert_eq!(outcome.passes, 1);
    assert_eq!(passes_in(&root).await[0].samples_within, 1);
}

/// Every sample that came within the radius is counted, not just the nearest.
#[tokio::test]
async fn the_samples_within_the_radius_are_counted() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    // Three samples 1 km apart, and a crossing between the first two: within a kilometre of
    // both of them and 1.5 km from the third.
    store_with_a_session(&root, 3).await;
    store_with_crossings(&root, &[500.0]).await;

    let outcome = crossings::silver::derive(&root, Radius::new(1_000.0))
        .await
        .expect("derive");

    assert_eq!(outcome.passes, 1);
    assert_eq!(passes_in(&root).await[0].samples_within, 2);
}
