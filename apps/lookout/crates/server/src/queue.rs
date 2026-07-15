//! The telemetry queue: samples are pushed onto a sink that the `recorder` cli
//! later drains. The production sink is an upstash redis list (`RedisSink`),
//! reached over TLS (`rediss://`) using the URL from `LOOKOUT_REDIS_URL`.
//!
//! `SampleSink` is the port the websocket handler pushes to, so tests can swap a
//! recording sink for redis and drive the real handler without a container.

use std::time::Duration;

use async_trait::async_trait;
use redis::aio::MultiplexedConnection;
use redis::{AsyncConnectionConfig, Client, RedisError};
use shared::Sample;

/// The redis list holding queued telemetry samples.
pub const QUEUE_KEY: &str = "lookout-telemetry";

/// redis's defaults (1s connect, 500ms response) are far too tight for a remote
/// Upstash TLS endpoint over the public internet; use generous timeouts that
/// still bound a genuine hang.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Failure enqueueing a sample.
#[derive(Debug, thiserror::Error)]
pub enum PushError {
    #[error("failed to serialize sample: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("redis error: {0}")]
    Redis(#[from] RedisError),
}

/// A destination the websocket handler enqueues received samples onto.
#[async_trait]
pub trait SampleSink: Send + Sync {
    /// Enqueue a sample, returning the resulting queue depth.
    async fn push(&self, sample: &Sample) -> Result<i64, PushError>;
}

/// A [`SampleSink`] backed by an upstash redis list.
pub struct RedisSink {
    conn: MultiplexedConnection,
}

impl RedisSink {
    /// Open a multiplexed connection to the telemetry redis.
    ///
    /// A `rediss://` URL negotiates TLS via rustls; the caller must have installed
    /// a rustls crypto provider first (see the server binary's startup).
    pub async fn connect(url: &str) -> Result<Self, RedisError> {
        let client = Client::open(url)?;
        let config = AsyncConnectionConfig::new()
            .set_connection_timeout(TIMEOUT)
            .set_response_timeout(TIMEOUT);
        let conn = client
            .get_multiplexed_async_connection_with_config(&config)
            .await?;
        Ok(Self { conn })
    }
}

#[async_trait]
impl SampleSink for RedisSink {
    async fn push(&self, sample: &Sample) -> Result<i64, PushError> {
        // MultiplexedConnection is a cheap, cloneable handle to one shared
        // connection; clone per push so concurrent sockets don't need a lock.
        let mut conn = self.conn.clone();
        let json = serde_json::to_string(sample)?;
        let depth: i64 = redis::cmd("LPUSH")
            .arg(QUEUE_KEY)
            .arg(json)
            .query_async(&mut conn)
            .await?;
        Ok(depth)
    }
}
