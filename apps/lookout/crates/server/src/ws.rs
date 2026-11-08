use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use shared::Message as TelemetryMessage;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use telemetry::RawSample;
use tokio::time::{Instant, interval_at};
use tower_sessions::Session;
use tower_sessions::session::Id;

use crate::AppState;
use crate::auth::{AuthError, RelyingParty};

pub async fn upgrade(
    State(state): State<AppState>,
    headers: HeaderMap,
    session: Session,
    upgrade: WebSocketUpgrade,
) -> Result<Response, AuthError> {
    if !state.relying_party.serves(headers.get(header::ORIGIN)) {
        tracing::warn!("refused a websocket upgrade from another origin");
        return Ok(StatusCode::FORBIDDEN.into_response());
    }
    let signed_in = state.relying_party.signed_in(&session).await?;
    Ok(match (signed_in, session.id()) {
        (Some(_), Some(id)) => upgrade.on_upgrade(move |socket| handle_socket(socket, state, id)),
        _ => {
            tracing::info!("refused a websocket upgrade without a signed-in session");
            StatusCode::UNAUTHORIZED.into_response()
        }
    })
}

async fn handle_socket(mut socket: WebSocket, state: AppState, session: Id) {
    tracing::info!("websocket connected");
    let mut recheck = interval_at(Instant::now() + SESSION_RECHECK, SESSION_RECHECK);
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
                if let Some(frame) = closing(&state.relying_party, &session).await {
                    if let Err(err) = socket.send(Message::Close(Some(frame))).await {
                        tracing::warn!(%err, "the close frame failed to send");
                    }
                    break;
                }
            }
        }
    }
    tracing::info!("websocket disconnected");
}

const SESSION_RECHECK: Duration = Duration::from_secs(60);
const SIGNED_OUT: &str = "the session is no longer signed in";
const UNCHECKED: &str = "the session could not be rechecked";

async fn closing(relying_party: &RelyingParty, session: &Id) -> Option<CloseFrame> {
    match relying_party.stored_session_signed_in(session).await {
        Ok(Some(_)) => None,
        Ok(None) => {
            tracing::info!("closing a socket whose session is no longer signed in");
            Some(CloseFrame {
                code: close_code::POLICY,
                reason: SIGNED_OUT.into(),
            })
        }
        Err(err) => {
            tracing::error!(%err, "closing a socket whose session could not be rechecked");
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
