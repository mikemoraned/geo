//! Integration test for the queue read paths against a real redis: the recorder's
//! two modes rely on `latest_samples` being newest-first + non-destructive, and
//! `brpop_sample` being FIFO + destructive. Samples are pushed the way the server
//! does (`LPUSH` of JSON).
//!
//! Requires Docker; the `_docker`-suffixed name is skipped by the no-docker profile.

use std::time::Duration;

use redis::aio::MultiplexedConnection;
use shared::{Accel, AccelReading, Message, V1Message};
use telemetry::{QUEUE_KEY, RawSample, brpop_sample, latest_samples};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::redis::{REDIS_PORT, Redis};
use uuid::Uuid;

async fn start_redis() -> (ContainerAsync<Redis>, String) {
    // Pin a modern redis: `brpop_sample` uses a fractional BRPOP timeout, which
    // only Redis >= 6.0 accepts (the module's default image is older). upstash,
    // the real backend, is modern.
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

fn sample(n: u128) -> Message {
    Message::Version1(V1Message::Acceleration(AccelReading {
        id: Uuid::from_u128(n),
        t: 1_700_000_000_000 + n as i64,
        accel: Accel {
            rms: 0.0,
            peak: 0.0,
            n: 1,
            x: Some(n as f64),
            y: None,
            z: None,
        },
    }))
}

/// LPUSH a message the way the server's `RedisSink` does: as a RawSample envelope
/// (payload + received_at).
async fn lpush(conn: &mut MultiplexedConnection, sample: &Message) {
    let payload = serde_json::to_string(sample).expect("serialize");
    let item = serde_json::to_string(&RawSample::new(1_700_000_050_000, payload))
        .expect("serialize envelope");
    let _: i64 = redis::cmd("LPUSH")
        .arg(QUEUE_KEY)
        .arg(item)
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
    let latest: Vec<Message> = latest_samples(&mut conn, 2)
        .await
        .expect("latest_samples")
        .iter()
        .map(|raw| raw.parse().expect("parse"))
        .collect();
    assert_eq!(latest, vec![sample(3), sample(2)]);
    let len: i64 = redis::cmd("LLEN")
        .arg(QUEUE_KEY)
        .query_async(&mut conn)
        .await
        .expect("llen");
    assert_eq!(len, 3, "view-latest must not remove anything");

    // drain: FIFO (BRPOP from the tail = oldest first), destructive.
    let mut popped = Vec::new();
    while let Some(raw) = brpop_sample(&mut conn, Duration::from_secs(2))
        .await
        .expect("brpop")
    {
        popped.push(raw.parse().expect("parse"));
    }
    assert_eq!(popped, vec![sample(1), sample(2), sample(3)]);

    // Empty queue: BRPOP times out → None.
    let empty = brpop_sample(&mut conn, Duration::from_millis(200))
        .await
        .expect("brpop empty");
    assert_eq!(empty, None);
}
