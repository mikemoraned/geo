//! Integration test for the telemetry websocket, exercising the real router and a
//! real websocket client end-to-end. A recording [`SampleSink`] stands in for redis
//! so the whole `/ws` path — connect, receive, parse, enqueue — is driven without a
//! container. (The redis adapter itself is covered separately against a real redis.)

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::SinkExt;
use server::queue::{PushError, SampleSink};
use server::{build_app, AppState};
use shared::{Accel, Sample};
use tokio::net::TcpListener;
use tokio::time::{sleep, Instant};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

/// A [`SampleSink`] that records pushed samples in memory for assertions.
struct RecordingSink {
    samples: Arc<Mutex<Vec<Sample>>>,
}

#[async_trait::async_trait]
impl SampleSink for RecordingSink {
    async fn push(&self, sample: &Sample) -> Result<i64, PushError> {
        let mut samples = self.samples.lock().expect("lock");
        samples.push(sample.clone());
        Ok(samples.len() as i64)
    }
}

fn static_dir() -> String {
    concat!(env!("CARGO_MANIFEST_DIR"), "/static").to_string()
}

#[tokio::test]
async fn valid_sample_is_enqueued_and_malformed_is_dropped() {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let sink = RecordingSink {
        samples: Arc::clone(&recorded),
    };
    let app = build_app(
        AppState {
            sink: Some(Arc::new(sink)),
        },
        static_dir(),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    let sample = Sample {
        id: Uuid::from_u128(42),
        t: 1_700_000_000_042,
        gps: None,
        accel: Some(Accel {
            x: Some(0.1),
            y: Some(-9.8),
            z: Some(0.3),
        }),
    };
    let json = serde_json::to_string(&sample).expect("serialize");

    let (mut ws, _resp) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("connect");
    // Malformed first, valid second: once the valid one is recorded we know the
    // malformed one has already been processed (and dropped) by the in-order handler.
    ws.send(Message::Text("not-a-sample".into()))
        .await
        .expect("send malformed");
    ws.send(Message::Text(json.into()))
        .await
        .expect("send valid");
    ws.close(None).await.expect("close");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        {
            let samples = recorded.lock().expect("lock");
            if !samples.is_empty() {
                assert_eq!(samples.len(), 1, "malformed sample must be dropped");
                assert_eq!(samples[0], sample);
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "sample never reached the sink within 5s"
        );
        sleep(Duration::from_millis(50)).await;
    }
}
