//! The telemetry queue: an upstash redis list the `server` pushes samples onto and
//! the `recorder` cli drains. This crate owns the wire contract shared by both — the
//! queue key, how to connect over TLS, and how to read a sample back off — so neither
//! side hard-codes it. The push adapter lives with the server (its only pusher).

use std::time::Duration;

use redis::aio::MultiplexedConnection;
use redis::{AsyncConnectionConfig, Client, RedisError};
use serde::{Deserialize, Serialize};
use shared::Message;

/// The redis list holding queued telemetry samples.
pub const QUEUE_KEY: &str = "lookout-telemetry";

/// One item on the queue: a verbatim sample payload plus `received_at`, the epoch
/// millis the **server** stamped when it first received the sample over the websocket.
///
/// `received_at` is set at handling time, not when the recorder later drains the
/// queue, so queue latency doesn't distort it (device clocks drift, and `t` inside the
/// payload is device-stamped). It rides *beside* the payload rather than inside it, so
/// the payload — and the md5 an archive keys on — stay verbatim. `parse` decodes the
/// payload into the typed [`Message`] for derived, per-sensor views.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawSample {
    received_at: i64,
    payload: String,
}

impl RawSample {
    /// Wrap a queue payload with the time the server received it, without validating
    /// the payload (validation is deferred to `parse`, so an unparseable payload can
    /// still be archived losslessly).
    pub fn new(received_at: i64, payload: impl Into<String>) -> Self {
        Self {
            received_at,
            payload: payload.into(),
        }
    }

    /// Epoch millis the server received this sample.
    pub fn received_at(&self) -> i64 {
        self.received_at
    }

    /// The raw JSON payload, exactly as queued.
    pub fn json(&self) -> &str {
        &self.payload
    }

    /// Decode the payload into a typed [`Message`].
    pub fn parse(&self) -> Result<Message, serde_json::Error> {
        serde_json::from_str(&self.payload)
    }
}

/// Failure reading a sample off the queue.
#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("redis error: {0}")]
    Redis(#[from] RedisError),
    #[error("malformed queue item: {0}")]
    Malformed(#[from] serde_json::Error),
}

/// redis's defaults (1s connect, 500ms response) are far too tight for a remote
/// Upstash TLS endpoint over the public internet; use generous timeouts that still
/// bound a genuine hang.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Open a multiplexed connection to the telemetry redis.
///
/// A `rediss://` URL negotiates TLS via rustls; the caller must have installed a
/// rustls crypto provider first (see each binary's startup).
pub async fn connect(url: &str) -> Result<MultiplexedConnection, RedisError> {
    let client = Client::open(url)?;
    let config = AsyncConnectionConfig::new()
        .set_connection_timeout(TIMEOUT)
        .set_response_timeout(TIMEOUT);
    client
        .get_multiplexed_async_connection_with_config(&config)
        .await
}

/// `BRPOP` one sample off the tail of the queue (oldest first — FIFO), blocking up to
/// `timeout`. Returns `None` when the timeout elapses with the queue still empty.
pub async fn brpop_sample(
    conn: &mut MultiplexedConnection,
    timeout: Duration,
) -> Result<Option<RawSample>, QueueError> {
    let popped: Option<(String, String)> = redis::cmd("BRPOP")
        .arg(QUEUE_KEY)
        .arg(timeout.as_secs_f64())
        .query_async(conn)
        .await?;
    popped
        .map(|(_key, item)| serde_json::from_str(&item))
        .transpose()
        .map_err(QueueError::from)
}

/// Put a sample back on the **tail** of the queue — where `BRPOP` took it — so a
/// failed archive doesn't destroy it. `RPUSH` targets the tail, restoring its FIFO
/// position ahead of the head-pushed newer samples.
pub async fn requeue_sample(
    conn: &mut MultiplexedConnection,
    sample: &RawSample,
) -> Result<(), QueueError> {
    let item = serde_json::to_string(sample)?;
    let _: i64 = redis::cmd("RPUSH")
        .arg(QUEUE_KEY)
        .arg(item)
        .query_async(conn)
        .await?;
    Ok(())
}

/// Read the most recent `limit` samples **without removing them** (non-destructive).
/// `LPUSH` puts the newest sample at the head, so index 0 is newest; the returned
/// vec is newest-first.
pub async fn latest_samples(
    conn: &mut MultiplexedConnection,
    limit: usize,
) -> Result<Vec<RawSample>, QueueError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    // LRANGE indices are inclusive: 0..=limit-1 is the newest `limit` entries.
    let items: Vec<String> = redis::cmd("LRANGE")
        .arg(QUEUE_KEY)
        .arg(0)
        .arg(limit as isize - 1)
        .query_async(conn)
        .await?;
    items
        .iter()
        .map(|item| serde_json::from_str(item))
        .collect::<Result<Vec<_>, _>>()
        .map_err(QueueError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// upstash is TLS-only, so we connect with `rediss://`. redis only parses that
    /// scheme when the crate is built with TLS features; without them `Client::open`
    /// fails with InvalidClientConfig and the server silently falls back to log-only
    /// (this shipped once). No network — parsing the URL is enough to prove the
    /// features are present.
    #[test]
    fn rediss_urls_parse() {
        redis::Client::open("rediss://default:secret@example.upstash.io:6379")
            .expect("rediss:// must parse — needs redis TLS features enabled");
    }

    /// The queue item is an envelope carrying the payload verbatim beside the
    /// server-stamped receive time; both survive the round-trip through the queue.
    #[test]
    fn raw_sample_envelope_roundtrips() {
        let payload = r#"{"v":1,"type":"gps","id":"00000000-0000-0000-0000-000000000001","t":1700000000000,"gps":{"lat":55.95,"lon":-3.19,"alt":null,"acc":8.5,"speed":31.4,"heading":null}}"#;
        let sample = RawSample::new(1_700_000_050_000, payload);

        let item = serde_json::to_string(&sample).expect("serialize");
        let decoded: RawSample = serde_json::from_str(&item).expect("deserialize");

        assert_eq!(decoded, sample);
        assert_eq!(decoded.received_at(), 1_700_000_050_000);
        assert_eq!(decoded.json(), payload);
        assert!(decoded.parse().is_ok(), "payload stays parseable");
    }
}
