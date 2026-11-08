pub mod admin;
pub mod code;
mod contract;
pub mod fresh;
pub mod gate;
pub mod memory;
pub mod redis;
pub mod registration;
pub mod sessions;
pub mod signin;
pub mod signout;
pub mod status;
pub mod store;
mod test_passkeys;

use std::fmt;
use std::sync::Arc;

use ::redis::aio::MultiplexedConnection;
use axum::Router;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use tower_sessions::session::Id;
use tower_sessions::{MemoryStore, Session, SessionManagerLayer};
use uuid::Uuid;
use webauthn_rs::prelude::{CredentialID, Url, Webauthn, WebauthnBuilder, WebauthnError};

use crate::auth::memory::MemoryAuthStore;
use crate::auth::redis::RedisAuthStore;
use crate::auth::sessions::redis::RedisSessionStore;
use crate::auth::sessions::{SIGNED_IN_PASSKEY, SharedSessionStore, session_layer};
use crate::auth::store::{AllowedPasskey, AuthStore, CeremonyId, StoreError};

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

    pub fn over_redis(
        origin: &Url,
        conn: MultiplexedConnection,
    ) -> Result<Self, RelyingPartyError> {
        Self::new(
            origin,
            Arc::new(RedisAuthStore::new(conn.clone())),
            SharedSessionStore::new(RedisSessionStore::new(conn)),
            Allowing::ByAdmin,
        )
    }

    pub async fn signed_in(&self, session: &Session) -> Result<Option<AllowedPasskey>, AuthError> {
        let Some(id) = session.get::<CredentialID>(SIGNED_IN_PASSKEY).await? else {
            return Ok(None);
        };
        Ok(self.store.find_allowed(&id).await?)
    }

    pub async fn stored_session_signed_in(
        &self,
        session: &Id,
    ) -> Result<Option<AllowedPasskey>, AuthError> {
        let stored = Session::new(Some(*session), Arc::new(self.sessions.clone()), None);
        self.signed_in(&stored).await
    }

    pub fn serves(&self, origin: Option<&HeaderValue>) -> bool {
        origin.is_some_and(|origin| origin == self.origin.origin().ascii_serialization().as_str())
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
        .route("/auth/signout", post(signout::signout))
        .route("/auth/status", get(status::status))
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
