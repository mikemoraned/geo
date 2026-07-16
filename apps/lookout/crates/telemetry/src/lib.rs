//! The telemetry queue: an upstash redis list the `server` pushes samples onto and
//! the `recorder` cli drains. This crate owns the wire contract shared by both — the
//! queue key, how to connect over TLS, and how to read a sample back off — so neither
//! side hard-codes it. The push adapter lives with the server (its only pusher).

use std::time::Duration;

use redis::aio::MultiplexedConnection;
use redis::{AsyncConnectionConfig, Client, RedisError};
use shared::Message;

/// The redis list holding queued telemetry samples.
pub const QUEUE_KEY: &str = "lookout-telemetry";

/// The raw JSON payload of one queued sample, exactly as it sits on the queue.
///
/// The queue is the lossless source of truth, so the bytes are preserved verbatim
/// (an archive keyed on their hash must match what was sent, not a re-serialization).
/// `parse` decodes them into the typed [`Message`] for derived, per-sensor views.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSample(String);

impl RawSample {
    /// Wrap a queue JSON payload without validating it (validation is deferred to
    /// `parse`, so an unparseable payload can still be archived losslessly).
    pub fn new(json: impl Into<String>) -> Self {
        Self(json.into())
    }

    /// The raw JSON payload, exactly as queued.
    pub fn json(&self) -> &str {
        &self.0
    }

    /// Decode the payload into a typed [`Message`].
    pub fn parse(&self) -> Result<Message, serde_json::Error> {
        serde_json::from_str(&self.0)
    }
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
) -> Result<Option<RawSample>, RedisError> {
    let popped: Option<(String, String)> = redis::cmd("BRPOP")
        .arg(QUEUE_KEY)
        .arg(timeout.as_secs_f64())
        .query_async(conn)
        .await?;
    Ok(popped.map(|(_key, json)| RawSample::new(json)))
}

/// Read the most recent `limit` samples **without removing them** (non-destructive).
/// `LPUSH` puts the newest sample at the head, so index 0 is newest; the returned
/// vec is newest-first.
pub async fn latest_samples(
    conn: &mut MultiplexedConnection,
    limit: usize,
) -> Result<Vec<RawSample>, RedisError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    // LRANGE indices are inclusive: 0..=limit-1 is the newest `limit` entries.
    let jsons: Vec<String> = redis::cmd("LRANGE")
        .arg(QUEUE_KEY)
        .arg(0)
        .arg(limit as isize - 1)
        .query_async(conn)
        .await?;
    Ok(jsons.into_iter().map(RawSample::new).collect())
}

#[cfg(test)]
mod tests {
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
}
