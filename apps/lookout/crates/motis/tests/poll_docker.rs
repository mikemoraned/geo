//! Integration test for the poll core [`motis::poll::poll_once`]: GPS samples on a real
//! redis (testcontainers) drive a real [`MotisClient`] against a mock Motis server
//! (wiremock) and land in a real [`Store`]. Exercises the whole tick — recent-GPS
//! filtering, the buffered-bbox query, and appending segments — end to end.
//!
//! Requires Docker (redis); the `_docker`-suffixed name is skipped by the no-docker
//! profile. The Motis server is mocked, so no live Motis is needed.

use std::time::Duration;

use chrono::Utc;
use motis::client::MotisClient;
use motis::poll::{poll_once, PollConfig, PollOutcome};
use motis::store::Store;
use motis::window::PositionWindow;
use redis::aio::MultiplexedConnection;
use shared::{Accel, AccelReading, Gps, GpsReading, Message, V1Message};
use telemetry::{RawSample, QUEUE_KEY};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::redis::{Redis, REDIS_PORT};
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The captured real 4-segment, mode-varied fixture (rail/subway/tram/bus).
const TRIPS_FIXTURE: &str = include_str!("fixtures/trips.json");
/// A captured real `trip` itinerary; its first leg names `DB Regio AG Mitte Region Hessen`.
const TRIP_FIXTURE: &str = include_str!("fixtures/trip.json");

async fn start_redis() -> (ContainerAsync<Redis>, String) {
    let container = Redis::default()
        .with_tag("7-alpine")
        .start()
        .await
        .expect("start redis");
    let host = container.get_host().await.expect("host");
    let port = container
        .get_host_port_ipv4(REDIS_PORT)
        .await
        .expect("port");
    (container, format!("redis://{host}:{port}"))
}

/// Wait until redis answers a `PING` (the host port-forward can lag `start()`).
async fn wait_ready(url: &str) -> MultiplexedConnection {
    let client = redis::Client::open(url).expect("open client");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(mut conn) = client.get_multiplexed_async_connection().await {
            if redis::cmd("PING")
                .query_async::<String>(&mut conn)
                .await
                .is_ok()
            {
                return conn;
            }
        }
        assert!(std::time::Instant::now() < deadline, "redis not ready in 30s");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn gps(id: u128, t: i64, lat: f64, lon: f64) -> Message {
    Message::Version1(V1Message::Gps(GpsReading {
        id: Uuid::from_u128(id),
        t,
        gps: Gps {
            lat,
            lon,
            alt: None,
            acc: 5.0,
            speed: None,
            heading: None,
        },
    }))
}

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

/// LPUSH a message the way the server's `RedisSink` does: as a RawSample envelope.
async fn lpush(conn: &mut MultiplexedConnection, message: &Message) {
    let payload = serde_json::to_string(message).expect("serialize message");
    let item = serde_json::to_string(&RawSample::new(1_700_000_050_000, payload))
        .expect("serialize envelope");
    let _: i64 = redis::cmd("LPUSH")
        .arg(QUEUE_KEY)
        .arg(item)
        .query_async(conn)
        .await
        .expect("lpush");
}

async fn mock_motis(segments_json: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v4/map/trips"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(segments_json.as_bytes(), "application/json"))
        .expect(1)
        .mount(&server)
        .await;
    // Every `trip` lookup (one per distinct tripId) resolves to the same itinerary.
    Mock::given(method("GET"))
        .and(path("/api/v4/trip"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(TRIP_FIXTURE.as_bytes(), "application/json"))
        .mount(&server)
        .await;
    server
}

fn fixture_len() -> usize {
    serde_json::from_str::<serde_json::Value>(TRIPS_FIXTURE)
        .expect("parse fixture")
        .as_array()
        .expect("fixture is an array")
        .len()
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

    let db = tempfile::NamedTempFile::new().expect("temp db");
    let store = Store::open(db.path()).expect("open store");
    let client = MotisClient::new(&motis.uri());
    let mut window = PositionWindow::new(Duration::from_secs(30 * 60));
    let config = PollConfig {
        recent_lookback: Duration::from_secs(5 * 60),
        query_window_half: Duration::from_secs(5 * 60),
        zoom: 8.0,
        sample_limit: 1000,
    };

    let outcome = poll_once(now, &mut conn, &client, &store, &mut window, &config)
        .await
        .expect("poll once");

    // The accel sample and the 20-min-old GPS are excluded; the three recent GPS are
    // ingested and their bbox queried, returning the four fixture segments.
    assert_eq!(
        outcome,
        PollOutcome::Queried {
            ingested: 3,
            positions: 3,
            segments: fixture_len(),
        }
    );

    // The segments are persisted to the on-disk db, each with its resolved agency.
    let reopened = rusqlite::Connection::open(db.path()).expect("reopen db");
    let count: i64 = reopened
        .query_row("SELECT COUNT(*) FROM segment", [], |r| r.get(0))
        .expect("count");
    assert_eq!(count as usize, fixture_len());
    let with_agency: i64 = reopened
        .query_row(
            "SELECT COUNT(*) FROM segment WHERE agency_name = 'DB Regio AG Mitte Region Hessen'",
            [],
            |r| r.get(0),
        )
        .expect("count agency");
    assert_eq!(with_agency as usize, fixture_len(), "every segment's trip resolved an agency");
}
