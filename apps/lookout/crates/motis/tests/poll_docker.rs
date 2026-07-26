//! Integration test for the poll core [`motis::poll::poll_once`]: GPS samples on a real
//! redis (testcontainers) drive a real [`MotisClient`] against a mock Motis server
//! (wiremock) and land in a real bronze capture log. Exercises the whole tick — recent-GPS
//! filtering, the buffered-bbox query, and writing the poll's segments — end to end.
//!
//! Requires Docker (redis); the `_docker`-suffixed name is skipped by the no-docker
//! profile. The Motis server is mocked, so no live Motis is needed.

mod common;

use std::time::Duration;

use chrono::Utc;
use common::{captured_segments, gps, lpush, start_redis, wait_ready, RAIL_MODES};
use medallion::Root;
use motis::bronze::SegmentLog;
use motis::client::MotisClient;
use motis::poll::{poll_once, PollConfig, PollOutcome};
use motis::window::PositionWindow;
use shared::{Accel, AccelReading, Message, V1Message};
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The captured real 4-segment, mode-varied fixture (rail/subway/tram/bus). The poll
/// filters to rail, so only the one rail segment is kept.
const TRIPS_FIXTURE: &str = include_str!("fixtures/trips.json");
/// A captured real long-distance `trip` itinerary: `DB Fernverkehr AG`, train number 2569.
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
        .and(path("/api/v4/map/trips"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(segments_json.as_bytes(), "application/json"),
        )
        .expect(1)
        .mount(&server)
        .await;
    // Every `trip` lookup (one per distinct tripId) resolves to the same itinerary.
    Mock::given(method("GET"))
        .and(path("/api/v4/trip"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(TRIP_FIXTURE.as_bytes(), "application/json"),
        )
        .mount(&server)
        .await;
    server
}

/// How many fixture segments are rail — the count the poll keeps after filtering.
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
async fn poll_once_ingests_recent_gps_and_logs_motis_segments_docker() {
    let (_container, url) = start_redis().await;
    let mut conn = wait_ready(&url).await;

    let now = Utc::now();
    let now_ms = now.timestamp_millis();
    // Pushed oldest→newest (LPUSH prepends, so index 0 ends up newest).
    lpush(&mut conn, &accel(1, now_ms)).await; // not GPS → ignored
    lpush(&mut conn, &gps(2, now_ms - 20 * 60_000, 50.0, 8.5)).await; // >5min old → filtered
    lpush(&mut conn, &gps(3, now_ms - 2 * 60_000, 50.106, 8.662)).await;
    lpush(&mut conn, &gps(4, now_ms - 60_000, 50.133, 8.741)).await;
    lpush(&mut conn, &gps(5, now_ms, 50.122, 8.710)).await;

    let motis = mock_motis(TRIPS_FIXTURE).await;

    let store = tempfile::tempdir().expect("temp store");
    let log = SegmentLog::new(Root::new(store.path()));
    let client = MotisClient::new(&motis.uri());
    let mut window = PositionWindow::new(Duration::from_secs(30 * 60));
    let config = PollConfig {
        recent_lookback: Duration::from_secs(5 * 60),
        query_window_half: Duration::from_secs(5 * 60),
        zoom: 8.0,
        sample_limit: 1000,
    };

    let outcome = poll_once(now, &mut conn, &client, &log, &mut window, &config)
        .await
        .expect("poll once");

    // The accel sample and the 20-min-old GPS are excluded; the three recent GPS are
    // ingested and their bbox queried, returning four segments of which only the one rail
    // segment survives the filter.
    assert_eq!(
        outcome,
        PollOutcome::Queried {
            ingested: 3,
            positions: 3,
            segments: rail_fixture_len(),
        }
    );

    // Only the rail segment is persisted, with its resolved agency and train number.
    let captured = captured_segments(&log.poll_file(now).expect("poll file"));
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
