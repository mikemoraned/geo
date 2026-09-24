use std::time::Duration;

use redis::aio::MultiplexedConnection;
use shared::{Accel, AccelReading, Message, V1Message};
use telemetry::{QUEUE_KEY, RawSample, peek_newest_samples, take_oldest_sample};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::redis::{REDIS_PORT, Redis};
use uuid::Uuid;

const FRACTIONAL_TIMEOUT_TAG: &str = "7-alpine";

async fn start_redis() -> (ContainerAsync<Redis>, String) {
    let container = Redis::default()
        .with_tag(FRACTIONAL_TIMEOUT_TAG)
        .start()
        .await
        .expect("start redis");
    let host = container.get_host().await.expect("host");
    let port = container
        .get_host_port_ipv4(REDIS_PORT)
        .await
        .expect("port");
    (container, format!("redis://{host}:{port}"))
}

async fn wait_ready(url: &str) -> MultiplexedConnection {
    let client = redis::Client::open(url).expect("open client");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
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
            std::time::Instant::now() < deadline,
            "redis not ready in 30s"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn sample(n: u128) -> Message {
    Message::Version1(V1Message::Acceleration(AccelReading {
        id: Uuid::from_u128(n),
        t: 1_700_000_000_000 + n as i64,
        accel: Accel {
            rms: 0.0,
            peak: 0.0,
            n: 1,
            x: Some(n as f64),
            y: None,
            z: None,
        },
    }))
}

async fn push_as_a_receiver_does(conn: &mut MultiplexedConnection, sample: &Message) {
    let payload = serde_json::to_string(sample).expect("serialize");
    let item = serde_json::to_string(&RawSample::new(1_700_000_050_000, payload))
        .expect("serialize envelope");
    let _: i64 = redis::cmd("LPUSH")
        .arg(QUEUE_KEY)
        .arg(item)
        .query_async(conn)
        .await
        .expect("lpush");
}

async fn queued_length(conn: &mut MultiplexedConnection) -> i64 {
    redis::cmd("LLEN")
        .arg(QUEUE_KEY)
        .query_async(conn)
        .await
        .expect("llen")
}

async fn peeking_answers_newest_first_and_removes_nothing(conn: &mut MultiplexedConnection) {
    let newest: Vec<Message> = peek_newest_samples(conn, 2)
        .await
        .expect("peek_newest_samples")
        .iter()
        .map(|raw| raw.parse().expect("parse"))
        .collect();

    assert_eq!(newest, vec![sample(3), sample(2)]);
    assert_eq!(queued_length(conn).await, 3);
}

async fn taking_answers_oldest_first_and_empties_the_queue(conn: &mut MultiplexedConnection) {
    let mut taken = Vec::new();
    while let Some(raw) = take_oldest_sample(conn, Duration::from_secs(2))
        .await
        .expect("take_oldest_sample")
    {
        taken.push(raw.parse().expect("parse"));
    }

    assert_eq!(taken, vec![sample(1), sample(2), sample(3)]);
    assert_eq!(queued_length(conn).await, 0);
}

async fn taking_from_an_empty_queue_answers_nothing(conn: &mut MultiplexedConnection) {
    let taken = take_oldest_sample(conn, Duration::from_millis(200))
        .await
        .expect("take_oldest_sample");

    assert_eq!(taken, None);
}

#[tokio::test]
async fn queue_read_paths_docker() {
    let (_container, url) = start_redis().await;
    let mut conn = wait_ready(&url).await;
    for n in 1..=3 {
        push_as_a_receiver_does(&mut conn, &sample(n)).await;
    }

    peeking_answers_newest_first_and_removes_nothing(&mut conn).await;
    taking_answers_oldest_first_and_empties_the_queue(&mut conn).await;
    taking_from_an_empty_queue_answers_nothing(&mut conn).await;
}
