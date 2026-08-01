//! End-to-end integration test for the recorder's extract path: prefill a real redis
//! (a testcontainer) the way the server does (`LPUSH` of sample JSON), run the actual
//! `recorder` binary to drain it into a medallion store, then query the store and assert
//! it holds the lossless raw rows plus the readings interpreted from them.
//!
//! Requires Docker; the `_docker`-suffixed name is skipped by the no-docker profile.

use std::process::Command;
use std::time::Duration;

use medallion::{Query, Root};
use redis::aio::MultiplexedConnection;
use serde::Deserialize;
use shared::{Accel, AccelReading, Gps, GpsReading, Message, V1Message};
use telemetry::{RawSample, QUEUE_KEY};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::redis::{Redis, REDIS_PORT};
use uuid::Uuid;

async fn start_redis() -> (ContainerAsync<Redis>, String) {
    // Match the queue test's pin: a fractional BRPOP timeout needs Redis >= 6.0.
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
        if let Ok(mut conn) = client.get_multiplexed_async_connection().await
            && redis::cmd("PING")
                .query_async::<String>(&mut conn)
                .await
                .is_ok()
            {
                return conn;
            }
        assert!(
            std::time::Instant::now() < deadline,
            "redis not ready in 30s"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// LPUSH a message the way the server's `RedisSink` does: as a RawSample envelope
/// (payload + received_at).
async fn lpush(conn: &mut MultiplexedConnection, message: &Message) {
    let payload = serde_json::to_string(message).expect("serialize");
    let item = serde_json::to_string(&RawSample::new(1_700_000_050_000, payload))
        .expect("serialize envelope");
    let _: i64 = redis::cmd("LPUSH")
        .arg(QUEUE_KEY)
        .arg(item)
        .query_async(conn)
        .await
        .expect("lpush");
}

fn accel_sample(id: Uuid, t: i64) -> Message {
    Message::Version1(V1Message::Acceleration(AccelReading {
        id,
        t,
        accel: Accel {
            rms: 0.42,
            peak: 1.7,
            n: 600,
            x: Some(0.1),
            y: Some(-9.8),
            z: Some(0.3),
        },
    }))
}

fn gps_sample(id: Uuid, t: i64, lat: f64) -> Message {
    Message::Version1(V1Message::Gps(GpsReading {
        id,
        t,
        gps: Gps {
            lat,
            lon: -3.19,
            alt: Some(80.0),
            acc: 5.0,
            speed: Some(31.4),
            heading: Some(275.0),
        },
    }))
}

/// One row of the gps dataset, as the assertions need it.
#[derive(Debug, Deserialize)]
struct Fix {
    lat: f64,
}

#[tokio::test]
async fn extract_queue_to_store_docker() {
    let (_container, url) = start_redis().await;
    let mut conn = wait_ready(&url).await;

    // Prefill the queue: three accel readings and two gps readings for one device.
    let device = Uuid::from_u128(1);
    lpush(&mut conn, &accel_sample(device, 1_700_000_000_000)).await;
    lpush(&mut conn, &accel_sample(device, 1_700_000_000_001)).await;
    lpush(&mut conn, &accel_sample(device, 1_700_000_000_002)).await;
    lpush(&mut conn, &gps_sample(device, 1_700_000_000_003, 55.95)).await;
    lpush(&mut conn, &gps_sample(device, 1_700_000_000_004, 55.96)).await;

    // Extract via the real recorder binary, pointed at this redis, draining into a
    // throwaway store.
    let dir = tempfile::tempdir().expect("tempdir");
    let status = Command::new(env!("CARGO_BIN_EXE_recorder"))
        .args(["drain", "--medallion-root"])
        .arg(dir.path())
        .env("LOOKOUT_REDIS_URL", &url)
        .status()
        .expect("run recorder");
    assert!(status.success(), "recorder exited with {status}");

    // Query the store and assert it holds the lossless payloads plus the readings
    // interpreted from them.
    let query = Query::new(Root::new(dir.path()));
    for dataset in [model::RAW_SAMPLE, model::GPS_READING, model::ACCEL_READING] {
        query
            .register(dataset, dataset.name)
            .await
            .expect("register dataset");
    }
    let mut counts = Vec::new();
    for dataset in [model::RAW_SAMPLE, model::ACCEL_READING, model::GPS_READING] {
        counts.push(
            query
                .count(&format!("SELECT COUNT(*) AS count FROM {}", dataset.name))
                .await
                .expect("count"),
        );
    }
    assert_eq!(
        counts,
        vec![5, 3, 2],
        "one lossless row per queued payload, and the readings interpreted from them"
    );

    let fixes: Vec<Fix> = query
        .rows(&format!(
            "SELECT lat FROM {} ORDER BY t",
            model::GPS_READING.name
        ))
        .await
        .expect("gps fixes");
    assert_eq!(
        fixes.iter().map(|f| f.lat).collect::<Vec<_>>(),
        vec![55.95, 55.96]
    );
}
