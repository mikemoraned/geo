use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use webauthn_rs::prelude::{
    CredentialID, DiscoverableAuthentication, Passkey, PasskeyRegistration, Uuid,
};

use crate::auth::code::ShortCode;

pub const CEREMONY_LIFETIME: Duration = Duration::from_secs(10 * 60);
pub const PENDING_LIFETIME: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CeremonyId(Uuid);

impl CeremonyId {
    pub fn random() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for CeremonyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Ceremony {
    Registration(PasskeyRegistration),
    SignIn(DiscoverableAuthentication),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPasskey {
    pub passkey: Passkey,
    pub registered_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllowedPasskey {
    pub passkey: Passkey,
    pub code: ShortCode,
    pub label: Option<String>,
    pub registered_at: DateTime<Utc>,
    pub allowed_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("redis failed: {0}")]
    Redis(#[from] redis::RedisError),
    #[error("a stored record failed to encode or decode: {0}")]
    Record(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Insertion {
    Inserted,
    CodeTaken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Update {
    Updated,
    Gone,
}

#[async_trait]
pub trait AuthStore: Send + Sync {
    async fn put_ceremony(&self, id: CeremonyId, ceremony: Ceremony) -> Result<(), StoreError>;
    async fn take_ceremony(&self, id: CeremonyId) -> Result<Option<Ceremony>, StoreError>;
    async fn insert_pending(
        &self,
        code: &ShortCode,
        passkey: PendingPasskey,
    ) -> Result<Insertion, StoreError>;
    async fn take_pending(&self, code: &ShortCode) -> Result<Option<PendingPasskey>, StoreError>;
    async fn find_allowed(&self, id: &CredentialID) -> Result<Option<AllowedPasskey>, StoreError>;
    async fn put_allowed(&self, passkey: AllowedPasskey) -> Result<(), StoreError>;
    async fn update_allowed(&self, passkey: AllowedPasskey) -> Result<Update, StoreError>;
    async fn list_allowed(&self) -> Result<Vec<AllowedPasskey>, StoreError>;
    async fn remove_allowed(&self, code: &ShortCode) -> Result<Option<AllowedPasskey>, StoreError>;

    async fn put_pending(&self, passkey: PendingPasskey) -> Result<ShortCode, StoreError> {
        loop {
            let code = ShortCode::random();
            if self.insert_pending(&code, passkey.clone()).await? == Insertion::Inserted {
                return Ok(code);
            }
        }
    }

    async fn allow(
        &self,
        code: &ShortCode,
        label: Option<String>,
    ) -> Result<Option<AllowedPasskey>, StoreError> {
        let Some(pending) = self.take_pending(code).await? else {
            return Ok(None);
        };
        let allowed = AllowedPasskey {
            passkey: pending.passkey,
            code: code.clone(),
            label,
            registered_at: pending.registered_at,
            allowed_at: Utc::now(),
            last_used_at: None,
        };
        self.put_allowed(allowed.clone()).await?;
        Ok(Some(allowed))
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn different_ceremonies_show_differently(a in any::<u128>(), b in any::<u128>()) {
            prop_assume!(a != b);

            prop_assert_ne!(
                CeremonyId(Uuid::from_u128(a)).to_string(),
                CeremonyId(Uuid::from_u128(b)).to_string()
            );
        }
    }
}
