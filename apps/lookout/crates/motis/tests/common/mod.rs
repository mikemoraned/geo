use domain::Gps;
use redis::AsyncCommands;
use redis::aio::MultiplexedConnection;
use shared::{GpsReading, Message, V1Message};
use telemetry::{QUEUE_KEY, RawSample};
use url::Url;
use uuid::Uuid;

pub const RAIL_MODES: [&str; 6] = [
    "HIGHSPEED_RAIL",
    "LONG_DISTANCE",
    "NIGHT_RAIL",
    "REGIONAL_FAST_RAIL",
    "REGIONAL_RAIL",
    "RAIL",
];

pub fn gps(id: u128, t: i64, lat: f64, lon: f64) -> Message {
    Message::Version1(V1Message::Gps(GpsReading {
        id: Uuid::from_u128(id),
        t,
        gps: Gps::at(lat, lon)
            .expect("on the globe")
            .with_accuracy_metres(Some(5.0)),
    }))
}

pub async fn lpush(conn: &mut MultiplexedConnection, message: &Message) {
    let payload = serde_json::to_string(message).expect("serialize message");
    let item = serde_json::to_string(&RawSample::new(1_700_000_050_000, payload))
        .expect("serialize envelope");
    let _: i64 = conn.lpush(QUEUE_KEY, item).await.expect("lpush");
}

#[derive(Debug, serde::Deserialize)]
pub struct CapturedSegment {
    pub mode: String,
    pub agency_name: Option<String>,
    pub train_number: Option<u32>,
}

pub async fn captured_segments(root: &medallion::Root) -> Vec<CapturedSegment> {
    let query = medallion::Query::new(root.clone());
    query
        .register(medallion_model::MOTIS_POLL_V2, "captured")
        .await
        .expect("register capture log");
    query
        .rows("SELECT mode, agency_name, train_number FROM captured")
        .await
        .expect("read capture log")
}

pub fn local_source() -> medallion_model::MotisSource {
    medallion_model::MotisSource {
        base_url: Url::parse(motis::client::DEFAULT_BASE_URL).expect("a URL"),
        motis_version: "v2.11.3".parse().expect("a version"),
        feed: medallion_model::Feed::Local,
        area: None,
        label: None,
    }
}
