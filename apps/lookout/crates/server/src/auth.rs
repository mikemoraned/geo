pub mod code;
pub mod memory;
pub mod registration;
pub mod sessions;
pub mod signin;
pub mod store;

#[cfg(test)]
mod test_passkeys;

use std::fmt;
use std::sync::Arc;

use axum::Router;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde::{Deserialize, Serialize};
use tower_sessions::{MemoryStore, SessionManagerLayer};
use uuid::Uuid;
use webauthn_rs::prelude::{Url, Webauthn, WebauthnBuilder, WebauthnError};

use crate::auth::memory::MemoryAuthStore;
use crate::auth::sessions::{SharedSessionStore, session_layer};
use crate::auth::store::{AuthStore, CeremonyId, StoreError};

const ACCOUNT: &str = "recording";

fn auth_namespace() -> Uuid {
    Uuid::new_v5(&Uuid::nil(), b"AUTH_NAMESPACE")
}

fn account_handle() -> Uuid {
    Uuid::new_v5(&auth_namespace(), ACCOUNT.as_bytes())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Allowing {
    ByAdmin,
    AtOnce,
}

#[derive(Clone)]
pub struct RelyingParty {
    origin: Url,
    webauthn: Arc<Webauthn>,
    store: Arc<dyn AuthStore>,
    sessions: SharedSessionStore,
    allowing: Allowing,
}

#[derive(Debug, thiserror::Error)]
pub enum RelyingPartyError {
    #[error("{0} has no domain to serve as the relying party ID")]
    NoDomain(Url),
    #[error("webauthn refused the relying party: {0}")]
    Webauthn(#[from] WebauthnError),
}

impl RelyingParty {
    pub fn new(
        origin: &Url,
        store: Arc<dyn AuthStore>,
        sessions: SharedSessionStore,
        allowing: Allowing,
    ) -> Result<Self, RelyingPartyError> {
        let id = origin
            .domain()
            .ok_or_else(|| RelyingPartyError::NoDomain(origin.clone()))?;
        let webauthn = WebauthnBuilder::new(id, origin)?.build()?;
        Ok(Self {
            origin: origin.clone(),
            webauthn: Arc::new(webauthn),
            store,
            sessions,
            allowing,
        })
    }

    pub fn in_memory(origin: &Url) -> Result<Self, RelyingPartyError> {
        let allowing = match origin.domain() {
            Some("localhost") => Allowing::AtOnce,
            _ => Allowing::ByAdmin,
        };
        Self::new(
            origin,
            Arc::new(MemoryAuthStore::default()),
            SharedSessionStore::new(MemoryStore::default()),
            allowing,
        )
    }

    pub fn session_layer(&self) -> SessionManagerLayer<SharedSessionStore> {
        session_layer(&self.origin, self.sessions.clone())
    }
}

impl fmt::Debug for RelyingParty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelyingParty")
            .field("origin", &self.origin)
            .field("webauthn", &self.webauthn)
            .field("allowing", &self.allowing)
            .finish_non_exhaustive()
    }
}

pub fn routes<S>(relying_party: RelyingParty) -> Router<S> {
    Router::new()
        .route("/auth/register/start", post(registration::start))
        .route("/auth/register/finish", post(registration::finish))
        .route("/auth/signin/start", post(signin::start))
        .route("/auth/signin/finish", post(signin::finish))
        .with_state(relying_party)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Started<Options> {
    pub ceremony: CeremonyId,
    pub options: Options,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finish<Credential> {
    pub ceremony: CeremonyId,
    pub credential: Credential,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("the ceremony expired, finished already, or never started")]
    NoCeremony,
    #[error("the credential failed verification: {0}")]
    Unverified(WebauthnError),
    #[error("the passkey is not allowed")]
    NotAllowed,
    #[error("the ceremony failed to start: {0}")]
    Unstarted(WebauthnError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("the session failed: {0}")]
    Session(#[from] tower_sessions::session::Error),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        match self {
            AuthError::NoCeremony | AuthError::Unverified(_) => {
                tracing::warn!(err = %self, "refused an auth request");
                (StatusCode::BAD_REQUEST, self.to_string()).into_response()
            }
            AuthError::NotAllowed => {
                tracing::warn!(err = %self, "refused an auth request");
                (StatusCode::FORBIDDEN, self.to_string()).into_response()
            }
            AuthError::Unstarted(_) | AuthError::Store(_) | AuthError::Session(_) => {
                tracing::error!(err = %self, "an auth request failed");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}
