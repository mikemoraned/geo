use std::time::Duration;

use backon::{ConstantBuilder, Retryable};
use redis::AsyncCommands;
use redis::aio::MultiplexedConnection;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::redis::{REDIS_PORT, Redis};

const TAG: &str = "7-alpine";
const READY_WITHIN: Duration = Duration::from_secs(30);
const RETRY_EVERY: Duration = Duration::from_millis(100);
const TRIES: usize = (READY_WITHIN.as_millis() / RETRY_EVERY.as_millis()) as usize;

pub struct TestRedis {
    _container: ContainerAsync<Redis>,
    url: String,
    conn: MultiplexedConnection,
}

impl TestRedis {
    pub async fn start() -> Self {
        let container = Redis::default()
            .with_tag(TAG)
            .start()
            .await
            .expect("a redis container");
        let host = container.get_host().await.expect("a host");
        let port = container
            .get_host_port_ipv4(REDIS_PORT)
            .await
            .expect("a mapped port");
        let url = format!("redis://{host}:{port}");
        let conn = ready(&url).await;
        Self {
            _container: container,
            url,
            conn,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn connection(&self) -> MultiplexedConnection {
        self.conn.clone()
    }

    pub async fn seconds_to_live(&self, key: &str) -> i64 {
        self.connection().ttl(key).await.expect("a TTL")
    }
}

async fn ready(url: &str) -> MultiplexedConnection {
    let client = redis::Client::open(url).expect("a redis URL");
    let ping = || async {
        let mut conn = client.get_multiplexed_async_connection().await?;
        let _: String = conn.ping().await?;
        Ok::<_, redis::RedisError>(conn)
    };
    ping.retry(
        ConstantBuilder::new()
            .with_delay(RETRY_EVERY)
            .with_max_times(TRIES),
    )
    .await
    .unwrap_or_else(|err| panic!("redis not ready in {READY_WITHIN:?}: {err}"))
}
