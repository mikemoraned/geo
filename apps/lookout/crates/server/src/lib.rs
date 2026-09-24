pub mod queue;

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::{any, get};
use shared::Message as TelemetryMessage;
use std::time::{SystemTime, UNIX_EPOCH};
use telemetry::RawSample;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::queue::SampleSink;

pub const GIT_HASH: &str = env!("BUILD_GIT_HASH");

#[derive(Clone)]
pub struct AppState {
    pub sink: Option<Arc<dyn SampleSink>>,
}

pub fn build_app(state: AppState, static_dir: impl Into<String>) -> Router {
    Router::new()
        .route("/ws", any(ws_upgrade))
        .route("/version", get(version))
        .fallback_service(ServeDir::new(static_dir.into()))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn version() -> &'static str {
    GIT_HASH
}

async fn ws_upgrade(State(state): State<AppState>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    tracing::info!("websocket connected");
    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Text(text) => {
                if let Ingest::Accepted = handle_sample(&state, &text).await
                    && socket.send(Message::Text(ACK.into())).await.is_err()
                {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    tracing::info!("websocket disconnected");
}

const ACK: &str = "ack";

enum Ingest {
    Accepted,
    Retry,
}

async fn handle_sample(state: &AppState, text: &str) -> Ingest {
    let message: TelemetryMessage = match serde_json::from_str(text) {
        Ok(message) => message,
        Err(err) => {
            tracing::warn!(%err, %text, "discarding malformed sample; a re-send cannot fix it");
            return Ingest::Accepted;
        }
    };

    let sample = RawSample::new(received_at_millis(), text);
    match &state.sink {
        Some(sink) => match sink.push(&sample).await {
            Ok(depth) => {
                tracing::info!(id = %message.device_id(), t = message.captured_at_millis(), depth, "queued sample");
                Ingest::Accepted
            }
            Err(err) => {
                tracing::error!(%err, "failed to queue sample");
                Ingest::Retry
            }
        },
        None => {
            tracing::info!(id = %message.device_id(), t = message.captured_at_millis(), "sample (not queued)");
            Ingest::Accepted
        }
    }
}

fn received_at_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
