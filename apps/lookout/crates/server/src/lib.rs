pub mod queue;

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::{any, get};
use axum::Router;
use shared::Message as TelemetryMessage;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::queue::SampleSink;

/// The git commit this binary was built from, injected via the `BUILD_GIT_HASH`
/// build arg (or "unknown" for a bare `cargo build`). Logged at startup and served
/// at `/version` so a running deploy can be matched to its source.
pub const GIT_HASH: &str = env!("BUILD_GIT_HASH");

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
        .route("/version", get(version))
        .fallback_service(ServeDir::new(static_dir.into()))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// The build's git hash, for matching a running deploy to its source.
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
            Message::Text(text) => handle_sample(&state, &text).await,
            Message::Close(_) => break,
            _ => {}
        }
    }
    tracing::info!("websocket disconnected");
}

/// Validate an incoming message and, if a sink is configured, enqueue it.
async fn handle_sample(state: &AppState, text: &str) {
    let message: TelemetryMessage = match serde_json::from_str(text) {
        Ok(message) => message,
        Err(err) => {
            tracing::warn!(%err, %text, "discarding malformed sample");
            return;
        }
    };

    match &state.sink {
        Some(sink) => match sink.push(&message).await {
            Ok(depth) => {
                tracing::info!(id = %message.id(), t = message.t(), depth, "queued sample")
            }
            Err(err) => tracing::error!(%err, "failed to queue sample"),
        },
        None => tracing::info!(id = %message.id(), t = message.t(), "sample (not queued)"),
    }
}
