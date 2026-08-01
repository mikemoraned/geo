//! Integration test for the redis adapter (`RedisSink`) against a real redis.
//!
//! A testcontainers redis is started and samples are pushed through `RedisSink`,
//! then read back off the `lookout-telemetry` list the way the `recorder` cli will
//! (tail-first, FIFO) — covering LPUSH ordering, queue depth, and JSON round-trip
//! through redis.
//!
//! Requires Docker; the `_docker`-suffixed name marks it for exclusion from
//! no-docker test runs.

use std::time::Duration;

use redis::aio::MultiplexedConnection;
use server::queue::{RedisSink, SampleSink, QUEUE_KEY};
use shared::{Accel, AccelReading, Gps, GpsReading, Message, V1Message};
use telemetry::RawSample;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::redis::{Redis, REDIS_PORT};
use tokio::time::{sleep, Instant};
use uuid::Uuid;

async fn start_redis() -> (ContainerAsync<Redis>, String) {
    let container = Redis::default()
        .start()
        .await
        .expect("start redis container");
    let host = container.get_host().await.expect("get host");
    let port = container
        .get_host_port_ipv4(REDIS_PORT)
        .await
        .expect("get mapped port");
    (container, format!("redis://{host}:{port}"))
}

/// Wait until redis answers a `PING`.
///
/// The container's `start()` only blocks until the "Ready to accept connections"
/// log line, but Docker's host port-forward proxy can briefly still refuse
/// connections after that (especially on macOS), so a single connect is racy.
async fn wait_for_redis(url: &str) -> MultiplexedConnection {
    let client = redis::Client::open(url).expect("open redis client");
    let deadline = Instant::now() + Duration::from_secs(30);
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
            Instant::now() < deadline,
            "redis did not accept connections within 30s"
        );
        sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn redis_sink_lpushes_samples_in_fifo_order_docker() {
    let (_container, url) = start_redis().await;
    let mut conn = wait_for_redis(&url).await;

    let sink = RedisSink::connect(&url).await.expect("connect sink");

    let accel_sample = Message::Version1(V1Message::Acceleration(AccelReading {
        id: Uuid::from_u128(1),
        t: 1_700_000_000_001,
        accel: Accel {
            rms: 0.42,
            peak: 1.7,
            n: 600,
            x: Some(0.1),
            y: Some(-9.8),
            z: Some(0.3),
        },
    }));
    let gps_sample = Message::Version1(V1Message::Gps(GpsReading {
        id: Uuid::from_u128(2),
        t: 1_700_000_000_002,
        gps: Gps {
            lat: 55.95,
            lon: -3.19,
            alt: None,
            acc: 8.5,
            speed: Some(31.4),
            heading: Some(275.0),
        },
    }));

    // The sink enqueues RawSample envelopes (payload + received_at), the way the
    // server's handler builds them.
    let accel_item = wrap(&accel_sample);
    let gps_item = wrap(&gps_sample);

    // push returns the resulting queue depth
    assert_eq!(sink.push(&accel_item).await.expect("push accel"), 1);
    assert_eq!(sink.push(&gps_item).await.expect("push gps"), 2);

    let len: i64 = redis::cmd("LLEN")
        .arg(QUEUE_KEY)
        .query_async(&mut conn)
        .await
        .expect("llen");
    assert_eq!(len, 2);

    // The recorder drains with BRPOP (tail), and LPUSH prepends, so the tail is the
    // oldest sample — draining yields insertion order (FIFO).
    let first = rpop_sample(&mut conn).await;
    let second = rpop_sample(&mut conn).await;
    assert_eq!(first.parse().expect("parse accel"), accel_sample);
    assert_eq!(second.parse().expect("parse gps"), gps_sample);
}

fn wrap(message: &Message) -> RawSample {
    RawSample::new(
        1_700_000_050_000,
        serde_json::to_string(message).expect("serialize"),
    )
}

async fn rpop_sample(conn: &mut MultiplexedConnection) -> RawSample {
    let item: String = redis::cmd("RPOP")
        .arg(QUEUE_KEY)
        .query_async(conn)
        .await
        .expect("rpop");
    serde_json::from_str(&item).expect("deserialize envelope")
}
