use std::time::Duration;

use redis::AsyncCommands;
use redis::aio::MultiplexedConnection;
use shared::{Accel, AccelReading, Message, V1Message};
use telemetry::{QUEUE_KEY, RawSample, connect, peek_newest_samples, take_oldest_sample};
use test_redis::TestRedis;
use uuid::Uuid;

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
    let _: i64 = conn.lpush(QUEUE_KEY, item).await.expect("lpush");
}

async fn queued_length(conn: &mut MultiplexedConnection) -> i64 {
    conn.llen(QUEUE_KEY).await.expect("llen")
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
    let redis = TestRedis::start().await;
    let mut conn = connect(redis.url()).await.expect("a connection");
    for n in 1..=3 {
        push_as_a_receiver_does(&mut conn, &sample(n)).await;
    }

    peeking_answers_newest_first_and_removes_nothing(&mut conn).await;
    taking_answers_oldest_first_and_empties_the_queue(&mut conn).await;
    taking_from_an_empty_queue_answers_nothing(&mut conn).await;
}
