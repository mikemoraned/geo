use std::process::Command;
use std::time::Duration;

use domain::Gps;
use medallion::{Query, Root};
use redis::aio::MultiplexedConnection;
use serde::Deserialize;
use shared::{Accel, AccelReading, GpsReading, Message, V1Message};
use telemetry::{QUEUE_KEY, RawSample};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::redis::{REDIS_PORT, Redis};
use uuid::Uuid;

async fn start_redis() -> (ContainerAsync<Redis>, String) {
    let container = Redis::default()
        .with_tag("7-alpine")
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

async fn lpush(conn: &mut MultiplexedConnection, message: &Message) {
    let payload = serde_json::to_string(message).expect("serialize");
    let item = serde_json::to_string(&RawSample::new(1_700_000_050_000, payload))
        .expect("serialize envelope");
    let _: i64 = redis::cmd("LPUSH")
        .arg(QUEUE_KEY)
        .arg(item)
        .query_async(conn)
        .await
        .expect("lpush");
}

fn accel_sample(id: Uuid, t: i64) -> Message {
    Message::Version1(V1Message::Acceleration(AccelReading {
        id,
        t,
        accel: Accel {
            rms: 0.42,
            peak: 1.7,
            n: 600,
            x: Some(0.1),
            y: Some(-9.8),
            z: Some(0.3),
        },
    }))
}

fn gps_sample(id: Uuid, t: i64, lat: f64) -> Message {
    Message::Version1(V1Message::Gps(GpsReading {
        id,
        t,
        gps: Gps::at(lat, -3.19)
            .expect("on the globe")
            .with_altitude_metres(Some(80.0))
            .with_accuracy_metres(Some(5.0))
            .with_speed_mps(Some(31.4))
            .with_heading_degrees(Some(275.0)),
    }))
}

#[derive(Debug, Deserialize)]
struct Fix {
    lat: f64,
}

#[tokio::test]
async fn extract_queue_to_store_docker() {
    let (_container, url) = start_redis().await;
    let mut conn = wait_ready(&url).await;

    let device = Uuid::from_u128(1);
    lpush(&mut conn, &accel_sample(device, 1_700_000_000_000)).await;
    lpush(&mut conn, &accel_sample(device, 1_700_000_000_001)).await;
    lpush(&mut conn, &accel_sample(device, 1_700_000_000_002)).await;
    lpush(&mut conn, &gps_sample(device, 1_700_000_000_003, 55.95)).await;
    lpush(&mut conn, &gps_sample(device, 1_700_000_000_004, 55.96)).await;

    let dir = tempfile::tempdir().expect("tempdir");
    let status = Command::new(env!("CARGO_BIN_EXE_recorder"))
        .args(["drain", "--medallion-root"])
        .arg(dir.path())
        .env("LOOKOUT_REDIS_URL", &url)
        .status()
        .expect("run recorder");
    assert!(status.success(), "recorder exited with {status}");

    let query = Query::new(Root::new(dir.path()));
    for dataset in [
        medallion_model::RAW_SAMPLE,
        medallion_model::GPS_READING,
        medallion_model::ACCEL_READING,
    ] {
        query
            .register(dataset, dataset.name)
            .await
            .expect("register dataset");
    }
    let mut counts = Vec::new();
    for dataset in [
        medallion_model::RAW_SAMPLE,
        medallion_model::ACCEL_READING,
        medallion_model::GPS_READING,
    ] {
        counts.push(
            query
                .count(&format!("SELECT COUNT(*) AS count FROM {}", dataset.name))
                .await
                .expect("count"),
        );
    }
    assert_eq!(
        counts,
        vec![5, 3, 2],
        "one lossless row per queued payload, and the readings interpreted from them"
    );

    let fixes: Vec<Fix> = query
        .rows(&format!(
            "SELECT lat FROM {} ORDER BY t",
            medallion_model::GPS_READING.name
        ))
        .await
        .expect("gps fixes");
    assert_eq!(
        fixes.iter().map(|f| f.lat).collect::<Vec<_>>(),
        vec![55.95, 55.96]
    );
}
