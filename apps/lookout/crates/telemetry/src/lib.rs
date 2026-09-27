use std::time::Duration;

use redis::aio::MultiplexedConnection;
use redis::{AsyncConnectionConfig, Client, RedisError};
use serde::{Deserialize, Serialize};
use shared::Message;

pub const QUEUE_KEY: &str = "lookout-telemetry";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawSample {
    received_at: i64,
    payload: String,
}

impl RawSample {
    pub fn new(received_at: i64, payload: impl Into<String>) -> Self {
        Self {
            received_at,
            payload: payload.into(),
        }
    }

    pub fn received_at(&self) -> i64 {
        self.received_at
    }

    pub fn json(&self) -> &str {
        &self.payload
    }

    pub fn parse(&self) -> Result<Message, serde_json::Error> {
        serde_json::from_str(&self.payload)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("redis error: {0}")]
    Redis(#[from] RedisError),
    #[error("malformed queue item: {0}")]
    Malformed(#[from] serde_json::Error),
}

const TIMEOUT: Duration = Duration::from_secs(10);

/// A `rediss://` URL negotiates TLS through rustls, so the calling process installs a crypto
/// provider before the first connection.
pub async fn connect(url: &str) -> Result<MultiplexedConnection, RedisError> {
    let client = Client::open(url)?;
    let config = AsyncConnectionConfig::new()
        .set_connection_timeout(TIMEOUT)
        .set_response_timeout(TIMEOUT);
    client
        .get_multiplexed_async_connection_with_config(&config)
        .await
}

pub async fn take_oldest_sample(
    conn: &mut MultiplexedConnection,
    timeout: Duration,
) -> Result<Option<RawSample>, QueueError> {
    let taken: Option<(String, String)> = redis::cmd("BRPOP")
        .arg(QUEUE_KEY)
        .arg(timeout.as_secs_f64())
        .query_async(conn)
        .await?;
    taken
        .map(|(_key, item)| serde_json::from_str(&item))
        .transpose()
        .map_err(QueueError::from)
}

pub async fn requeue_as_oldest(
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

pub async fn peek_newest_samples(
    conn: &mut MultiplexedConnection,
    limit: usize,
) -> Result<Vec<RawSample>, QueueError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let last_index = limit as isize - 1;
    let items: Vec<String> = redis::cmd("LRANGE")
        .arg(QUEUE_KEY)
        .arg(0)
        .arg(last_index)
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

    #[test]
    fn rediss_urls_parse_with_the_tls_features_enabled() {
        redis::Client::open("rediss://default:secret@example.upstash.io:6379")
            .expect("rediss:// must parse — needs redis TLS features enabled");
    }

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
