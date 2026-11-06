pub mod auth;
pub mod queue;
pub mod stores;

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use shared::Message as TelemetryMessage;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use telemetry::RawSample;
use tokio::time::{Instant, interval_at};
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tower_sessions::Session;
use webauthn_rs::prelude::CredentialID;

use crate::auth::{AuthError, RelyingParty};
use crate::queue::SampleSink;

pub const GIT_HASH: &str = env!("BUILD_GIT_HASH");

#[derive(Clone)]
pub struct AppState {
    pub sink: Option<Arc<dyn SampleSink>>,
    pub relying_party: RelyingParty,
}

const RECORD_PAGE: &str = "/record";

pub fn build_app(state: AppState, static_dir: impl Into<String>) -> Router {
    let static_dir = static_dir.into();
    let record = Router::new()
        .nest_service(
            RECORD_PAGE,
            ServeDir::new(format!("{static_dir}{RECORD_PAGE}")),
        )
        .layer(middleware::from_fn_with_state(
            state.relying_party.clone(),
            auth::gate::require_signin,
        ));
    Router::new()
        .route("/ws", any(ws_upgrade))
        .route("/version", get(version))
        .merge(auth::routes(state.relying_party.clone()))
        .merge(record)
        .fallback_service(ServeDir::new(static_dir))
        .layer(state.relying_party.session_layer())
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn version() -> &'static str {
    GIT_HASH
}

async fn ws_upgrade(
    State(state): State<AppState>,
    headers: HeaderMap,
    session: Session,
    upgrade: WebSocketUpgrade,
) -> Result<Response, AuthError> {
    if !state.relying_party.serves(headers.get(header::ORIGIN)) {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }
    Ok(match state.relying_party.signed_in(&session).await? {
        Some(allowed) => {
            let passkey = allowed.passkey.cred_id().clone();
            upgrade.on_upgrade(move |socket| handle_socket(socket, state, passkey))
        }
        None => StatusCode::UNAUTHORIZED.into_response(),
    })
}

async fn handle_socket(mut socket: WebSocket, state: AppState, passkey: CredentialID) {
    tracing::info!("websocket connected");
    let mut recheck = interval_at(Instant::now() + PASSKEY_RECHECK, PASSKEY_RECHECK);
    loop {
        tokio::select! {
            received = socket.recv() => match received {
                Some(Ok(Message::Text(text))) => {
                    if let Ingest::Accepted = handle_sample(&state, &text).await
                        && socket.send(Message::Text(ACK.into())).await.is_err()
                    {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
            _ = recheck.tick() => {
                if let Some(frame) = closing(&state.relying_party, &passkey).await {
                    let _ = socket.send(Message::Close(Some(frame))).await;
                    break;
                }
            }
        }
    }
    tracing::info!("websocket disconnected");
}

const PASSKEY_RECHECK: Duration = Duration::from_secs(60);
const REVOKED: &str = "the passkey is no longer allowed";
const UNCHECKED: &str = "the passkey could not be rechecked";

async fn closing(relying_party: &RelyingParty, passkey: &CredentialID) -> Option<CloseFrame> {
    match relying_party.allowed(passkey).await {
        Ok(Some(_)) => None,
        Ok(None) => Some(CloseFrame {
            code: close_code::POLICY,
            reason: REVOKED.into(),
        }),
        Err(err) => {
            tracing::error!(%err, "closing a socket whose passkey could not be rechecked");
            Some(CloseFrame {
                code: close_code::ERROR,
                reason: UNCHECKED.into(),
            })
        }
    }
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
