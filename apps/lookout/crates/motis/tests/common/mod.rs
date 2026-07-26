//! Shared setup for the motis integration tests: a throwaway redis (testcontainers) and
//! helpers to seed GPS samples the way the server's `RedisSink` does.

use std::time::Duration;

use redis::aio::MultiplexedConnection;
use shared::{Gps, GpsReading, Message, V1Message};
use telemetry::{RawSample, QUEUE_KEY};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::redis::{Redis, REDIS_PORT};
use uuid::Uuid;

/// Motis `mode`s the poll keeps — mainline and regional rail.
pub const RAIL_MODES: [&str; 6] = [
    "HIGHSPEED_RAIL",
    "LONG_DISTANCE",
    "NIGHT_RAIL",
    "REGIONAL_FAST_RAIL",
    "REGIONAL_RAIL",
    "RAIL",
];

/// Start a throwaway redis container, returning it (drop = stop) and its URL.
pub async fn start_redis() -> (ContainerAsync<Redis>, String) {
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

/// Wait until redis answers a `PING` (the host port-forward can lag `start()`).
pub async fn wait_ready(url: &str) -> MultiplexedConnection {
    let client = redis::Client::open(url).expect("open client");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(mut conn) = client.get_multiplexed_async_connection().await {
            if redis::cmd("PING")
                .query_async::<String>(&mut conn)
                .await
                .is_ok()
            {
                return conn;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "redis not ready in 30s"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A GPS `Message` at time `t` (epoch ms) and position `(lat, lon)`.
pub fn gps(id: u128, t: i64, lat: f64, lon: f64) -> Message {
    Message::Version1(V1Message::Gps(GpsReading {
        id: Uuid::from_u128(id),
        t,
        gps: Gps {
            lat,
            lon,
            alt: None,
            acc: 5.0,
            speed: None,
            heading: None,
        },
    }))
}

/// LPUSH a message the way the server's `RedisSink` does: as a `RawSample` envelope.
pub async fn lpush(conn: &mut MultiplexedConnection, message: &Message) {
    let payload = serde_json::to_string(message).expect("serialize message");
    let item = serde_json::to_string(&RawSample::new(1_700_000_050_000, payload))
        .expect("serialize envelope");
    let _: i64 = redis::cmd("LPUSH")
        .arg(QUEUE_KEY)
        .arg(item)
        .query_async(conn)
        .await
        .expect("lpush");
}

/// One row of a bronze poll file, reduced to the fields the poll tests assert on.
#[derive(Debug)]
pub struct CapturedSegment {
    pub mode: String,
    pub agency_name: Option<String>,
    pub train_number: Option<i64>,
}

/// Read back the parquet file one poll wrote. Columns are cast to a single layout first,
/// so the assertions don't depend on which string or integer width the writer chose.
pub fn captured_segments(path: &std::path::Path) -> Vec<CapturedSegment> {
    use arrow::array::{Array, AsArray};
    use arrow::datatypes::{DataType, Int64Type};

    let file = std::fs::File::open(path).expect("open poll file");
    let reader = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(file)
        .expect("parquet")
        .build()
        .expect("reader");

    reader
        .flat_map(|batch| {
            let batch = batch.expect("batch");
            let text = |name: &str| {
                arrow::compute::cast(batch.column_by_name(name).expect(name), &DataType::Utf8)
                    .expect("cast to utf8")
            };
            let mode = text("mode");
            let agency = text("agency_name");
            let number = arrow::compute::cast(
                batch.column_by_name("train_number").expect("train_number"),
                &DataType::Int64,
            )
            .expect("cast to i64");

            (0..batch.num_rows())
                .map(|i| CapturedSegment {
                    mode: mode.as_string::<i32>().value(i).to_string(),
                    agency_name: agency
                        .is_valid(i)
                        .then(|| agency.as_string::<i32>().value(i).to_string()),
                    train_number: number
                        .is_valid(i)
                        .then(|| number.as_primitive::<Int64Type>().value(i)),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}
