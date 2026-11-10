pub mod redis;

use std::sync::Arc;

use async_trait::async_trait;
use tower_sessions::cookie::SameSite;
use tower_sessions::cookie::time::Duration;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{self, SessionStore};
use tower_sessions::{Expiry, SessionManagerLayer};
use webauthn_rs::prelude::Url;

pub const SIGNED_IN_PASSKEY: &str = "passkey";

const SECURE_COOKIE: &str = "__Host-lookout-session";
const PLAIN_COOKIE: &str = "lookout-session";
const IDLE_LIFETIME: Duration = Duration::days(30);

#[derive(Debug, Clone)]
pub struct SharedSessionStore(Arc<dyn SessionStore>);

impl SharedSessionStore {
    pub fn new(store: impl SessionStore) -> Self {
        Self(Arc::new(store))
    }
}

#[async_trait]
impl SessionStore for SharedSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        self.0.create(record).await
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        self.0.save(record).await
    }

    async fn load(&self, id: &Id) -> session_store::Result<Option<Record>> {
        self.0.load(id).await
    }

    async fn delete(&self, id: &Id) -> session_store::Result<()> {
        self.0.delete(id).await
    }
}

pub fn session_layer(
    origin: &Url,
    store: SharedSessionStore,
) -> SessionManagerLayer<SharedSessionStore> {
    let secure = origin.scheme() == "https";
    let name = if secure { SECURE_COOKIE } else { PLAIN_COOKIE };
    SessionManagerLayer::new(store)
        .with_name(name)
        .with_secure(secure)
        .with_http_only(true)
        .with_same_site(SameSite::Strict)
        .with_path("/")
        .with_expiry(Expiry::OnInactivity(IDLE_LIFETIME))
        .with_always_save(true)
}
