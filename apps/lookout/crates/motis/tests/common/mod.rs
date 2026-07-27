//! Shared setup for the motis integration tests: a throwaway redis (testcontainers) and
//! helpers to seed GPS samples the way the server's `RedisSink` does.

use std::time::Duration;

use redis::aio::MultiplexedConnection;
use shared::{Gps, GpsReading, Message, V1Message};
use telemetry::{RawSample, QUEUE_KEY};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::redis::{Redis, REDIS_PORT};
use uuid::Uuid;

/// Motis `mode`s the poll keeps — mainline and regional rail.
pub const RAIL_MODES: [&str; 6] = [
    "HIGHSPEED_RAIL",
    "LONG_DISTANCE",
    "NIGHT_RAIL",
    "REGIONAL_FAST_RAIL",
    "REGIONAL_RAIL",
    "RAIL",
];

/// Start a throwaway redis container, returning it (drop = stop) and its URL.
pub async fn start_redis() -> (ContainerAsync<Redis>, String) {
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
pub async fn wait_ready(url: &str) -> MultiplexedConnection {
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
        assert!(
            std::time::Instant::now() < deadline,
            "redis not ready in 30s"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A GPS `Message` at time `t` (epoch ms) and position `(lat, lon)`.
pub fn gps(id: u128, t: i64, lat: f64, lon: f64) -> Message {
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

/// LPUSH a message the way the server's `RedisSink` does: as a `RawSample` envelope.
pub async fn lpush(conn: &mut MultiplexedConnection, message: &Message) {
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

/// One row of the bronze capture log, reduced to the fields the poll tests assert on.
#[derive(Debug, serde::Deserialize)]
pub struct CapturedSegment {
    pub mode: String,
    pub agency_name: Option<String>,
    pub train_number: Option<u32>,
}

/// Read back what a poll captured, the way any other reader would: as a table.
pub async fn captured_segments(root: &medallion::Root) -> Vec<CapturedSegment> {
    let query = medallion::Query::new(root.clone());
    query
        .register(model::MOTIS_SEGMENT, "captured")
        .await
        .expect("register capture log");
    query
        .rows("SELECT mode, agency_name, train_number FROM captured")
        .await
        .expect("read capture log")
}
