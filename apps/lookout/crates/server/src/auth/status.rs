use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::signed_in::SignedIn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SignInStatus {
    SignedIn,
    SignedOut,
}

pub async fn status(signed_in: Option<SignedIn>) -> Json<SignInStatus> {
    let status = match signed_in {
        Some(_) => SignInStatus::SignedIn,
        None => SignInStatus::SignedOut,
    };
    tracing::info!(?status, "answered a sign-in status");
    Json(status)
}
