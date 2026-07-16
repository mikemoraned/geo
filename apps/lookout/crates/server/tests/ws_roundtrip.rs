//! Integration test for the telemetry websocket, exercising the real router and a
//! real websocket client end-to-end. A recording [`SampleSink`] stands in for redis
//! so the whole `/ws` path — connect, receive, parse, enqueue — is driven without a
//! container. (The redis adapter itself is covered separately against a real redis.)

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use server::queue::{PushError, SampleSink};
use server::{build_app, AppState};
use shared::{Accel, AccelReading, Gps, GpsReading, Message, V0Message, V1Message};
use telemetry::RawSample;
use tokio::net::TcpListener;
use tokio::time::{sleep, timeout, Instant};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

/// A [`SampleSink`] that records pushed queue items in memory for assertions.
struct RecordingSink {
    samples: Arc<Mutex<Vec<RawSample>>>,
}

#[async_trait::async_trait]
impl SampleSink for RecordingSink {
    async fn push(&self, sample: &RawSample) -> Result<i64, PushError> {
        let mut samples = self.samples.lock().expect("lock");
        samples.push(sample.clone());
        Ok(samples.len() as i64)
    }
}

fn static_dir() -> String {
    concat!(env!("CARGO_MANIFEST_DIR"), "/static").to_string()
}

/// Spawn the real router with a recording sink; returns its address and the shared
/// buffer of pushed queue items.
async fn spawn_app() -> (SocketAddr, Arc<Mutex<Vec<RawSample>>>) {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let app = build_app(
        AppState {
            sink: Some(Arc::new(RecordingSink {
                samples: Arc::clone(&recorded),
            })),
        },
        static_dir(),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (addr, recorded)
}

/// Both protocol versions must pass ingest: a historical v0 payload (no `v`, sensor
/// key inferred) and a modern v1 message (tagged), while genuinely malformed JSON is
/// dropped. The three are sent in order and the handler processes them in order, so
/// the two valid ones land in the sink and the malformed one doesn't.
#[tokio::test]
async fn both_versions_enqueue_and_malformed_is_dropped() {
    let (addr, recorded) = spawn_app().await;

    // A historical v0 payload: no `v`, variant inferred from the `gps` key.
    let v0_raw = r#"{"id":"00000000-0000-0000-0000-000000000007","t":1700000000007,"gps":{"lat":55.95,"lon":-3.19,"alt":null,"acc":8.5}}"#;
    let v0_expected = Message::Version0(V0Message::Gps(GpsReading {
        id: Uuid::from_u128(7),
        t: 1_700_000_000_007,
        gps: Gps {
            lat: 55.95,
            lon: -3.19,
            alt: None,
            acc: 8.5,
            speed: None,
            heading: None,
        },
    }));

    let v1 = Message::Version1(V1Message::Acceleration(AccelReading {
        id: Uuid::from_u128(42),
        t: 1_700_000_000_042,
        accel: Accel {
            rms: 0.42,
            peak: 1.7,
            n: 600,
            x: Some(0.1),
            y: Some(-9.8),
            z: Some(0.3),
        },
    }));
    let v1_json = serde_json::to_string(&v1).expect("serialize");

    let (mut ws, _resp) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("connect");
    // Malformed first so that, once both valid ones are recorded, we know the
    // malformed one was already processed (and dropped) by the in-order handler.
    ws.send(WsMessage::Text("not-a-sample".into()))
        .await
        .expect("send malformed");
    ws.send(WsMessage::Text(v0_raw.into()))
        .await
        .expect("send v0");
    ws.send(WsMessage::Text(v1_json.into()))
        .await
        .expect("send v1");
    ws.close(None).await.expect("close");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        {
            let samples = recorded.lock().expect("lock");
            if samples.len() >= 2 {
                assert_eq!(samples.len(), 2, "malformed sample must be dropped");
                assert_eq!(samples[0].parse().expect("parse v0"), v0_expected);
                assert_eq!(samples[1].parse().expect("parse v1"), v1);
                assert!(
                    samples[0].received_at() > 0,
                    "server stamps received_at at handling time"
                );
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "samples never reached the sink within 5s"
        );
        sleep(Duration::from_millis(50)).await;
    }
}

/// The server acks each accepted message so the client can drop it from its outbox
/// only once delivery is confirmed. Sending one sample must yield one ack frame back.
#[tokio::test]
async fn server_acks_accepted_message() {
    let (addr, _recorded) = spawn_app().await;

    let sample = Message::Version1(V1Message::Acceleration(AccelReading {
        id: Uuid::from_u128(99),
        t: 1_700_000_000_099,
        accel: Accel {
            rms: 0.0,
            peak: 0.0,
            n: 1,
            x: Some(0.0),
            y: Some(0.0),
            z: Some(0.0),
        },
    }));
    let json = serde_json::to_string(&sample).expect("serialize");

    let (mut ws, _resp) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("connect");
    ws.send(WsMessage::Text(json.into())).await.expect("send");

    let frame = timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("ack within 5s")
        .expect("stream open")
        .expect("frame");
    assert_eq!(frame, WsMessage::Text("ack".into()), "expected an ack frame");
}
