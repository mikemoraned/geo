//! Integration test for the queue read paths against a real redis: the recorder's
//! two modes rely on `latest_samples` being newest-first + non-destructive, and
//! `brpop_sample` being FIFO + destructive. Samples are pushed the way the server
//! does (`LPUSH` of JSON).
//!
//! Requires Docker; the `_docker`-suffixed name is skipped by the no-docker profile.

use std::time::Duration;

use redis::aio::MultiplexedConnection;
use shared::{Accel, Sample};
use telemetry::{brpop_sample, latest_samples, QUEUE_KEY};
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::redis::{Redis, REDIS_PORT};
use uuid::Uuid;

async fn start_redis() -> (ContainerAsync<Redis>, String) {
    let container = Redis::default().start().await.expect("start redis");
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

fn sample(n: u128) -> Sample {
    Sample {
        id: Uuid::from_u128(n),
        t: 1_700_000_000_000 + n as i64,
        gps: None,
        accel: Some(Accel {
            x: Some(n as f64),
            y: None,
            z: None,
        }),
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

#[tokio::test]
async fn queue_read_paths_docker() {
    let (_container, url) = start_redis().await;
    let mut conn = wait_ready(&url).await;

    // Push oldest→newest; LPUSH prepends, so the list head is newest (s3) and the
    // tail is oldest (s1).
    for n in 1..=3 {
        lpush(&mut conn, &sample(n)).await;
    }

    // view-latest: newest-first, limited, and non-destructive.
    let latest = latest_samples(&mut conn, 2).await.expect("latest_samples");
    assert_eq!(latest, vec![sample(3), sample(2)]);
    let len: i64 = redis::cmd("LLEN")
        .arg(QUEUE_KEY)
        .query_async(&mut conn)
        .await
        .expect("llen");
    assert_eq!(len, 3, "view-latest must not remove anything");

    // drain: FIFO (BRPOP from the tail = oldest first), destructive.
    let mut popped = Vec::new();
    while let Some(s) = brpop_sample(&mut conn, Duration::from_secs(2))
        .await
        .expect("brpop")
    {
        popped.push(s);
    }
    assert_eq!(popped, vec![sample(1), sample(2), sample(3)]);

    // Empty queue: BRPOP times out → None.
    let empty = brpop_sample(&mut conn, Duration::from_millis(200))
        .await
        .expect("brpop empty");
    assert_eq!(empty, None);
}
