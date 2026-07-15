//! End-to-end test against the real upstash redis, exercising the exact connect
//! path the server uses in production: `rediss://` URL parse, TLS handshake, and
//! auth — using the real credentials, not a synthetic URL. This is what catches
//! failures a local unit test can't (e.g. a malformed/whitespace-laden secret
//! value that fails `Client::open` with InvalidClientConfig).
//!
//! Requires real credentials and network; run via `just end_to_end_test`, which
//! injects `LOOKOUT_REDIS_URL` from 1Password and builds in `--release`.

use redis::aio::MultiplexedConnection;
use server::queue::{RedisSink, SampleSink, QUEUE_KEY};
use shared::{Accel, Sample};
use uuid::Uuid;

#[tokio::test]
async fn redis_round_trip_end_to_end() {
    // The same process-global TLS provider the server installs before connecting.
    rustls::crypto::ring::default_provider().install_default().ok();

    let url = std::env::var("LOOKOUT_REDIS_URL")
        .expect("LOOKOUT_REDIS_URL must be set — run via `just end_to_end_test`");

    // This is the call that fails in production; connecting proves parse + TLS + auth.
    let sink = RedisSink::connect(&url)
        .await
        .expect("connect to real upstash redis (parse + TLS + auth)");

    let sample = Sample {
        id: Uuid::new_v4(),
        t: 1_700_000_000_000,
        gps: None,
        accel: Some(Accel {
            x: Some(0.1),
            y: Some(-9.8),
            z: Some(0.3),
        }),
    };
    let json = serde_json::to_string(&sample).expect("serialize");

    let depth = sink.push(&sample).await.expect("push sample to upstash");
    assert!(depth >= 1, "queue depth should be >= 1 after push");

    // Remove exactly the sample we pushed — proves it landed on the queue and cleans
    // up after ourselves without draining anyone else's telemetry.
    let mut conn = raw_connection(&url).await;
    let removed: i64 = redis::cmd("LREM")
        .arg(QUEUE_KEY)
        .arg(1)
        .arg(&json)
        .query_async(&mut conn)
        .await
        .expect("LREM");
    assert_eq!(removed, 1, "the pushed sample must be found on the queue");
}

async fn raw_connection(url: &str) -> MultiplexedConnection {
    redis::Client::open(url)
        .expect("open redis client")
        .get_multiplexed_async_connection()
        .await
        .expect("connect raw redis")
}
