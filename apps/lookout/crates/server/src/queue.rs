use async_trait::async_trait;
use redis::aio::MultiplexedConnection;
use redis::{AsyncCommands, RedisError};
use telemetry::RawSample;

pub use telemetry::QUEUE_KEY;

#[derive(Debug, thiserror::Error)]
pub enum PushError {
    #[error("failed to serialize sample: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("redis error: {0}")]
    Redis(#[from] RedisError),
}

#[async_trait]
pub trait SampleSink: Send + Sync {
    async fn push(&self, sample: &RawSample) -> Result<i64, PushError>;
}

pub struct RedisSink {
    conn: MultiplexedConnection,
}

impl RedisSink {
    pub fn new(conn: MultiplexedConnection) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl SampleSink for RedisSink {
    async fn push(&self, sample: &RawSample) -> Result<i64, PushError> {
        let item = serde_json::to_string(sample)?;
        Ok(self.conn.clone().lpush(QUEUE_KEY, item).await?)
    }
}
