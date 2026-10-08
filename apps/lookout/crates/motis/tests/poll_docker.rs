mod common;

use chrono::{Duration, Utc};
use common::{RAIL_MODES, captured_segments, gps, lpush, start_redis, wait_ready};
use medallion::Root;
use motis::bronze::SegmentLog;
use motis::capture::{Capture, Captured};
use motis::client::MotisClient;
use motis::near_gps::NearGps;
use shared::{Accel, AccelReading, Message, V1Message};
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TRIPS_FIXTURE: &str = include_str!("fixtures/trips.json");
const TRIP_FIXTURE: &str = include_str!("fixtures/trip.json");

fn accel(id: u128, t: i64) -> Message {
    Message::Version1(V1Message::Acceleration(AccelReading {
        id: Uuid::from_u128(id),
        t,
        accel: Accel {
            rms: 0.0,
            peak: 0.0,
            n: 1,
            x: None,
            y: None,
            z: None,
        },
    }))
}

async fn mock_motis(segments_json: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v6/map/trips"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(segments_json.as_bytes(), "application/json"),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v6/trip"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(TRIP_FIXTURE.as_bytes(), "application/json"),
        )
        .mount(&server)
        .await;
    server
}

fn rail_fixture_len() -> usize {
    serde_json::from_str::<serde_json::Value>(TRIPS_FIXTURE)
        .expect("parse fixture")
        .as_array()
        .expect("fixture is an array")
        .iter()
        .filter(|s| RAIL_MODES.contains(&s["mode"].as_str().unwrap_or_default()))
        .count()
}

#[tokio::test]
async fn recent_gps_names_the_area_whose_motis_segments_are_logged_docker() {
    let (_container, url) = start_redis().await;
    let mut conn = wait_ready(&url).await;

    let now = Utc::now();
    let now_ms = now.timestamp_millis();
    let not_a_fix = accel(1, now_ms);
    let older_than_the_lookback = gps(2, now_ms - 20 * 60_000, 50.0, 8.5);
    lpush(&mut conn, &not_a_fix).await;
    lpush(&mut conn, &older_than_the_lookback).await;
    lpush(&mut conn, &gps(3, now_ms - 2 * 60_000, 50.106, 8.662)).await;
    lpush(&mut conn, &gps(4, now_ms - 60_000, 50.133, 8.741)).await;
    lpush(&mut conn, &gps(5, now_ms, 50.122, 8.710)).await;

    let motis = mock_motis(TRIPS_FIXTURE).await;

    let store = tempfile::tempdir().expect("temp store");
    let mut near_gps = NearGps::new(Duration::minutes(30), Duration::minutes(5), 1000);
    let mut capture = Capture::new(
        MotisClient::new(&motis.uri()),
        SegmentLog::new(Root::new(store.path()), common::local_source().id()),
        8.0,
        Duration::minutes(5),
    );

    let seen = near_gps.look(now, &mut conn).await.expect("look");
    assert_eq!((seen.ingested, seen.positions), (3, 3));
    let area = seen.area.expect("an area around recent GPS");
    let captured = capture.capture(now, &area).await.expect("capture");

    assert_eq!(
        captured,
        Captured {
            segments: rail_fixture_len(),
            unresolved: Vec::new(),
        }
    );

    let captured = captured_segments(&Root::new(store.path())).await;
    assert_eq!(captured.len(), rail_fixture_len());
    assert!(
        captured
            .iter()
            .all(|s| RAIL_MODES.contains(&s.mode.as_str())),
        "the non-rail fixture segments should have been filtered out; got {:?}",
        captured.iter().map(|s| s.mode.as_str()).collect::<Vec<_>>()
    );
    let enriched = captured
        .iter()
        .filter(|s| {
            s.agency_name.as_deref() == Some("DB Fernverkehr AG") && s.train_number == Some(2569)
        })
        .count();
    assert_eq!(
        enriched,
        rail_fixture_len(),
        "the rail segment's trip resolved its agency and train number"
    );
}
