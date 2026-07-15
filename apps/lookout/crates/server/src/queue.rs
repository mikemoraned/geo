//! The websocket handler's push side of the telemetry queue. `SampleSink` is the
//! port the handler pushes to, so tests can swap a recording sink for redis and
//! drive the real handler without a container. The production sink (`RedisSink`)
//! `LPUSH`es onto the shared queue; connection + key live in the `telemetry` crate.

use async_trait::async_trait;
use redis::aio::MultiplexedConnection;
use redis::RedisError;
use shared::Sample;

pub use telemetry::QUEUE_KEY;

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
    /// Connect to the telemetry redis. A `rediss://` URL negotiates TLS via rustls;
    /// the caller must have installed a rustls crypto provider first (see the server
    /// binary's startup).
    pub async fn connect(url: &str) -> Result<Self, RedisError> {
        let conn = telemetry::connect(url).await?;
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
