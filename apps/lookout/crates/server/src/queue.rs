use async_trait::async_trait;
use redis::RedisError;
use redis::aio::MultiplexedConnection;
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
    pub async fn connect(url: &str) -> Result<Self, RedisError> {
        let conn = telemetry::connect(url).await?;
        Ok(Self { conn })
    }
}

#[async_trait]
impl SampleSink for RedisSink {
    async fn push(&self, sample: &RawSample) -> Result<i64, PushError> {
        let mut conn = self.conn.clone();
        let item = serde_json::to_string(sample)?;
        let depth: i64 = redis::cmd("LPUSH")
            .arg(QUEUE_KEY)
            .arg(item)
            .query_async(&mut conn)
            .await?;
        Ok(depth)
    }
}
