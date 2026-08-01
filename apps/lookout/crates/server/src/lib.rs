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
            Message::Text(text) => {
                // Ack only once the server has taken responsibility, so the client
                // drops the message from its outbox; a mid-flush disconnect then
                // re-sends the un-acked tail instead of losing samples that looked
                // sent. Withhold the ack on a transient failure so it's retried.
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

/// The ack frame sent back per accepted message. The client treats any server
/// message as one ack (delivery is in-order over a single socket), so the payload
/// is a constant.
const ACK: &str = "ack";

/// Whether the server has finished with a received message. `Accepted` tells the
/// client (via an ack) it may drop the message; `Retry` withholds the ack so the
/// client re-sends it on reconnect.
enum Ingest {
    Accepted,
    Retry,
}

/// Validate an incoming message and, if a sink is configured, enqueue it. The queue
/// item carries the verbatim payload plus `received_at`, stamped here at handling time
/// so queue latency doesn't distort it.
async fn handle_sample(state: &AppState, text: &str) -> Ingest {
    let message: TelemetryMessage = match serde_json::from_str(text) {
        Ok(message) => message,
        Err(err) => {
            // Re-sending won't fix malformed JSON, so accept (drop) it rather than
            // blocking the client's outbox behind a message that can never succeed.
            tracing::warn!(%err, %text, "discarding malformed sample");
            return Ingest::Accepted;
        }
    };

    let sample = RawSample::new(received_at_millis(), text);
    match &state.sink {
        Some(sink) => match sink.push(&sample).await {
            Ok(depth) => {
                tracing::info!(id = %message.id(), t = message.t(), depth, "queued sample");
                Ingest::Accepted
            }
            Err(err) => {
                tracing::error!(%err, "failed to queue sample");
                Ingest::Retry
            }
        },
        None => {
            tracing::info!(id = %message.id(), t = message.t(), "sample (not queued)");
            Ingest::Accepted
        }
    }
}

/// Wall-clock time now, as epoch milliseconds — a server-stamped counterpart to the
/// device-stamped `t`. A backwards clock (pre-1970) saturates to 0 rather than
/// panicking on a single sample.
fn received_at_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
