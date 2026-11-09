use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

use crate::auth::{AuthError, RelyingParty};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SignInStatus {
    SignedIn,
    SignedOut,
}

pub async fn status(
    State(relying_party): State<RelyingParty>,
    session: Session,
) -> Result<Json<SignInStatus>, AuthError> {
    let status = if relying_party.signed_in(&session).await? {
        SignInStatus::SignedIn
    } else {
        SignInStatus::SignedOut
    };
    tracing::info!(?status, "answered a sign-in status");
    Ok(Json(status))
}
