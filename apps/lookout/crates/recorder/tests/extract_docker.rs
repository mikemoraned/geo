//! End-to-end integration test for the recorder's extract path: prefill a real redis
//! (a testcontainer) the way the server does (`LPUSH` of sample JSON), run the actual
//! `recorder` binary to drain it into a SQLite archive, then reopen the archive and
//! assert it holds the lossless raw rows plus the derived per-sensor rows.
//!
//! Requires Docker; the `_docker`-suffixed name is skipped by the no-docker profile.

use std::process::Command;
use std::time::Duration;

use redis::aio::MultiplexedConnection;
use shared::{Accel, Gps, Sample};
use telemetry::QUEUE_KEY;
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

/// LPUSH a sample the way the server's `RedisSink` does.
async fn lpush(conn: &mut MultiplexedConnection, sample: &Sample) {
    let json = serde_json::to_string(sample).expect("serialize");
    let _: i64 = redis::cmd("LPUSH")
        .arg(QUEUE_KEY)
        .arg(json)
        .query_async(conn)
        .await
        .expect("lpush");
}

fn accel_sample(id: Uuid, t: i64) -> Sample {
    Sample {
        id,
        t,
        gps: None,
        accel: Some(Accel {
            x: Some(0.1),
            y: Some(-9.8),
            z: Some(0.3),
        }),
    }
}

fn gps_sample(id: Uuid, t: i64, lat: f64) -> Sample {
    Sample {
        id,
        t,
        gps: Some(Gps {
            lat,
            lon: -3.19,
            alt: Some(80.0),
            acc: 5.0,
        }),
        accel: None,
    }
}

#[tokio::test]
async fn extract_queue_to_sqlite_docker() {
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
    // temp archive.
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("lookout.sqlite");
    let status = Command::new(env!("CARGO_BIN_EXE_recorder"))
        .args(["drain", "--output"])
        .arg(&db_path)
        .env("LOOKOUT_REDIS_URL", &url)
        .status()
        .expect("run recorder");
    assert!(status.success(), "recorder exited with {status}");

    // Reopen the archive and assert it holds the lossless raw rows plus the derived
    // per-sensor rows.
    let archive = rusqlite::Connection::open(&db_path).expect("open archive");
    let count = |table: &str| -> i64 {
        archive
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))
            .expect("count")
    };
    assert_eq!(count("raw"), 5, "one lossless row per queued payload");
    assert_eq!(count("accel"), 3);
    assert_eq!(count("gps"), 2);

    let lats: Vec<f64> = {
        let mut stmt = archive
            .prepare("SELECT lat FROM gps ORDER BY t")
            .expect("prepare");
        let rows = stmt
            .query_map([], |row| row.get(0))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("collect");
        rows
    };
    assert_eq!(lats, vec![55.95, 55.96]);
}
