pub mod queue;

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::any;
use axum::Router;
use shared::Sample;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::queue::SampleSink;

/// Shared handler state. `sink` is `None` when no telemetry sink is configured
/// (e.g. `LOOKOUT_REDIS_URL` unset), in which case received samples are logged
/// but not enqueued — the static site still serves, so the deploy isn't gated on
/// redis being configured.
#[derive(Clone)]
pub struct AppState {
    pub sink: Option<Arc<dyn SampleSink>>,
}

/// Build the router: `/ws` for telemetry, everything else served from `static_dir`.
pub fn build_app(state: AppState, static_dir: impl Into<String>) -> Router {
    Router::new()
        .route("/ws", any(ws_upgrade))
        .fallback_service(ServeDir::new(static_dir.into()))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn ws_upgrade(State(state): State<AppState>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    tracing::info!("websocket connected");
    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Text(text) => handle_sample(&state, &text).await,
            Message::Close(_) => break,
            _ => {}
        }
    }
    tracing::info!("websocket disconnected");
}

/// Validate an incoming sample and, if a sink is configured, enqueue it.
async fn handle_sample(state: &AppState, text: &str) {
    let sample: Sample = match serde_json::from_str(text) {
        Ok(sample) => sample,
        Err(err) => {
            tracing::warn!(%err, %text, "discarding malformed sample");
            return;
        }
    };

    match &state.sink {
        Some(sink) => match sink.push(&sample).await {
            Ok(depth) => tracing::info!(id = %sample.id, t = sample.t, depth, "queued sample"),
            Err(err) => tracing::error!(%err, "failed to queue sample"),
        },
        None => tracing::info!(id = %sample.id, t = sample.t, "sample (not queued)"),
    }
}
