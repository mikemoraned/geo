use std::time::Duration;

use redis::aio::MultiplexedConnection;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::redis::{REDIS_PORT, Redis};
use tokio::time::{Instant, sleep};

const TAG: &str = "7-alpine";
const READY_WITHIN: Duration = Duration::from_secs(30);
const RETRY_EVERY: Duration = Duration::from_millis(100);

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
        redis::cmd("TTL")
            .arg(key)
            .query_async(&mut self.connection())
            .await
            .expect("a TTL")
    }
}

async fn ready(url: &str) -> MultiplexedConnection {
    let client = redis::Client::open(url).expect("a redis URL");
    let deadline = Instant::now() + READY_WITHIN;
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
            "redis not ready in {READY_WITHIN:?}"
        );
        sleep(RETRY_EVERY).await;
    }
}
