use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio::time::Instant;
use webauthn_rs::prelude::CredentialID;

use crate::auth::code::ShortCode;
use crate::auth::store::{
    AllowedPasskey, AuthStore, CEREMONY_LIFETIME, Ceremony, CeremonyId, Insertion,
    PENDING_LIFETIME, PendingPasskey, StoreError, Update,
};

#[derive(Default)]
pub struct MemoryAuthStore {
    records: Mutex<Records>,
}

#[derive(Default)]
struct Records {
    ceremonies: HashMap<CeremonyId, Expiring<Ceremony>>,
    pending: HashMap<ShortCode, Expiring<PendingPasskey>>,
    allowed: HashMap<CredentialID, AllowedPasskey>,
}

struct Expiring<T> {
    value: T,
    expires_at: Instant,
}

impl<T> Expiring<T> {
    fn new(value: T, lifetime: Duration) -> Self {
        Self {
            value,
            expires_at: Instant::now() + lifetime,
        }
    }

    fn is_live(&self) -> bool {
        Instant::now() < self.expires_at
    }

    fn into_live(self) -> Option<T> {
        self.is_live().then_some(self.value)
    }
}

#[async_trait]
impl AuthStore for MemoryAuthStore {
    async fn put_ceremony(&self, id: CeremonyId, ceremony: Ceremony) -> Result<(), StoreError> {
        let mut records = self.records.lock().await;
        records.ceremonies.retain(|_, ceremony| ceremony.is_live());
        records
            .ceremonies
            .insert(id, Expiring::new(ceremony, CEREMONY_LIFETIME));
        Ok(())
    }

    async fn take_ceremony(&self, id: CeremonyId) -> Result<Option<Ceremony>, StoreError> {
        let mut records = self.records.lock().await;
        Ok(records.ceremonies.remove(&id).and_then(Expiring::into_live))
    }

    async fn insert_pending(
        &self,
        code: &ShortCode,
        passkey: PendingPasskey,
    ) -> Result<Insertion, StoreError> {
        let mut records = self.records.lock().await;
        records.pending.retain(|_, pending| pending.is_live());
        match records.pending.entry(code.clone()) {
            Entry::Occupied(_) => Ok(Insertion::CodeTaken),
            Entry::Vacant(slot) => {
                slot.insert(Expiring::new(passkey, PENDING_LIFETIME));
                Ok(Insertion::Inserted)
            }
        }
    }

    async fn take_pending(&self, code: &ShortCode) -> Result<Option<PendingPasskey>, StoreError> {
        let mut records = self.records.lock().await;
        Ok(records.pending.remove(code).and_then(Expiring::into_live))
    }

    async fn find_allowed(&self, id: &CredentialID) -> Result<Option<AllowedPasskey>, StoreError> {
        Ok(self.records.lock().await.allowed.get(id).cloned())
    }

    async fn put_allowed(&self, passkey: AllowedPasskey) -> Result<(), StoreError> {
        let id = passkey.passkey.cred_id().clone();
        self.records.lock().await.allowed.insert(id, passkey);
        Ok(())
    }

    async fn update_allowed(&self, passkey: AllowedPasskey) -> Result<Update, StoreError> {
        let mut records = self.records.lock().await;
        match records.allowed.get_mut(passkey.passkey.cred_id()) {
            Some(held) => {
                *held = passkey;
                Ok(Update::Updated)
            }
            None => Ok(Update::Gone),
        }
    }

    async fn list_allowed(&self) -> Result<Vec<AllowedPasskey>, StoreError> {
        Ok(self
            .records
            .lock()
            .await
            .allowed
            .values()
            .cloned()
            .collect())
    }

    async fn remove_allowed(&self, code: &ShortCode) -> Result<Option<AllowedPasskey>, StoreError> {
        let mut records = self.records.lock().await;
        let id = records
            .allowed
            .iter()
            .find(|(_, allowed)| &allowed.code == code)
            .map(|(id, _)| id.clone());
        Ok(id.and_then(|id| records.allowed.remove(&id)))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::advance;

    use super::*;
    use crate::auth::contract::{self, allowed_passkey, pending};
    use crate::auth::store::{CEREMONY_LIFETIME, PENDING_LIFETIME};
    use crate::auth::test_passkeys::{registered_passkey, registration_ceremony};

    const A_MOMENT: Duration = Duration::from_secs(1);

    #[tokio::test(start_paused = true)]
    async fn a_ceremony_is_taken_once() {
        contract::a_ceremony_is_taken_once(&MemoryAuthStore::default()).await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_pending_passkey_is_taken_once_by_its_code() {
        contract::a_pending_passkey_is_taken_once_by_its_code(&MemoryAuthStore::default()).await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_held_code_refuses_another_pending_passkey() {
        contract::a_held_code_refuses_another_pending_passkey(&MemoryAuthStore::default()).await;
    }

    #[tokio::test(start_paused = true)]
    async fn allowing_moves_a_pending_passkey_into_the_allowed_ones() {
        contract::allowing_moves_a_pending_passkey_into_the_allowed_ones(
            &MemoryAuthStore::default(),
        )
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn allowing_an_unknown_code_allows_nothing() {
        contract::allowing_an_unknown_code_allows_nothing(&MemoryAuthStore::default()).await;
    }

    #[tokio::test(start_paused = true)]
    async fn updating_rewrites_an_allowed_passkey() {
        contract::updating_rewrites_an_allowed_passkey(&MemoryAuthStore::default()).await;
    }

    #[tokio::test(start_paused = true)]
    async fn updating_a_revoked_passkey_leaves_it_revoked() {
        contract::updating_a_revoked_passkey_leaves_it_revoked(&MemoryAuthStore::default()).await;
    }

    #[tokio::test(start_paused = true)]
    async fn revoking_removes_only_the_passkey_with_that_code() {
        contract::revoking_removes_only_the_passkey_with_that_code(&MemoryAuthStore::default())
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn revoking_an_unknown_code_removes_nothing() {
        contract::revoking_an_unknown_code_removes_nothing(&MemoryAuthStore::default()).await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_ceremony_lasts_ten_minutes() {
        let store = MemoryAuthStore::default();
        let lasting = CeremonyId::random();
        let expired = CeremonyId::random();
        store
            .put_ceremony(lasting, registration_ceremony())
            .await
            .expect("stored");
        store
            .put_ceremony(expired, registration_ceremony())
            .await
            .expect("stored");

        advance(CEREMONY_LIFETIME - A_MOMENT).await;
        assert!(store.take_ceremony(lasting).await.expect("taken").is_some());
        advance(A_MOMENT * 2).await;
        assert!(store.take_ceremony(expired).await.expect("taken").is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn a_pending_passkey_lasts_an_hour() {
        let store = MemoryAuthStore::default();
        let lasting = store
            .put_pending(pending(registered_passkey().await))
            .await
            .expect("stored");
        let expired = store
            .put_pending(pending(registered_passkey().await))
            .await
            .expect("stored");

        advance(PENDING_LIFETIME - A_MOMENT).await;
        assert!(store.take_pending(&lasting).await.expect("taken").is_some());
        advance(A_MOMENT * 2).await;
        assert!(store.take_pending(&expired).await.expect("taken").is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn an_expired_pending_passkey_frees_its_code() {
        let store = MemoryAuthStore::default();
        let code = ShortCode::random();
        store
            .insert_pending(&code, pending(registered_passkey().await))
            .await
            .expect("inserted");

        advance(PENDING_LIFETIME + A_MOMENT).await;
        let reinserted = store
            .insert_pending(&code, pending(registered_passkey().await))
            .await
            .expect("inserted");

        assert_eq!(reinserted, Insertion::Inserted);
    }

    #[tokio::test(start_paused = true)]
    async fn an_allowed_passkey_outlasts_the_pending_hour() {
        let store = MemoryAuthStore::default();
        let allowed = allowed_passkey(&store).await;

        advance(PENDING_LIFETIME * 24).await;

        let found = store
            .find_allowed(allowed.passkey.cred_id())
            .await
            .expect("found");
        assert!(found.is_some());
    }
}
