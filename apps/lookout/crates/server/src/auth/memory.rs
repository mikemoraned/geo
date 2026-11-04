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

    use chrono::Utc;
    use tokio::time::advance;
    use webauthn_rs::prelude::Passkey;

    use super::*;
    use crate::auth::store::{CEREMONY_LIFETIME, PENDING_LIFETIME};
    use crate::auth::test_passkeys::{registered_passkey, registration_ceremony};

    const A_MOMENT: Duration = Duration::from_secs(1);

    fn pending(passkey: Passkey) -> PendingPasskey {
        PendingPasskey {
            passkey,
            registered_at: Utc::now(),
        }
    }

    async fn allowed_passkey(store: &MemoryAuthStore) -> AllowedPasskey {
        let code = store
            .put_pending(pending(registered_passkey().await))
            .await
            .expect("stored");
        store
            .allow(&code, None)
            .await
            .expect("allowed")
            .expect("a pending passkey")
    }

    #[tokio::test(start_paused = true)]
    async fn a_ceremony_is_taken_once() {
        let store = MemoryAuthStore::default();
        let id = CeremonyId::random();
        store
            .put_ceremony(id, registration_ceremony())
            .await
            .expect("stored");

        assert!(store.take_ceremony(id).await.expect("taken").is_some());
        assert!(store.take_ceremony(id).await.expect("taken").is_none());
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
    async fn a_pending_passkey_is_taken_once_by_its_code() {
        let store = MemoryAuthStore::default();
        let passkey = registered_passkey().await;
        let id = passkey.cred_id().clone();
        let code = store.put_pending(pending(passkey)).await.expect("stored");

        let taken = store.take_pending(&code).await.expect("taken");
        assert_eq!(taken.map(|p| p.passkey.cred_id().clone()), Some(id));
        assert!(store.take_pending(&code).await.expect("taken").is_none());
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
    async fn a_held_code_refuses_another_pending_passkey() {
        let store = MemoryAuthStore::default();
        let code = ShortCode::random();
        let first = pending(registered_passkey().await);
        let first_id = first.passkey.cred_id().clone();
        let second = pending(registered_passkey().await);

        let inserted = store.insert_pending(&code, first).await.expect("inserted");
        let refused = store.insert_pending(&code, second).await.expect("refused");

        assert_eq!(
            (inserted, refused),
            (Insertion::Inserted, Insertion::CodeTaken)
        );
        let held = store.take_pending(&code).await.expect("taken");
        assert_eq!(held.map(|p| p.passkey.cred_id().clone()), Some(first_id));
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
    async fn allowing_moves_a_pending_passkey_into_the_allowed_ones() {
        let store = MemoryAuthStore::default();
        let passkey = registered_passkey().await;
        let id = passkey.cred_id().clone();
        let code = store.put_pending(pending(passkey)).await.expect("stored");

        let allowed = store
            .allow(&code, Some("iPhone".into()))
            .await
            .expect("allowed")
            .expect("a pending passkey");

        assert_eq!(allowed.code, code);
        assert_eq!(allowed.label.as_deref(), Some("iPhone"));
        assert!(store.take_pending(&code).await.expect("taken").is_none());
        let found = store.find_allowed(&id).await.expect("found");
        assert_eq!(found.map(|a| a.code), Some(code));
    }

    #[tokio::test(start_paused = true)]
    async fn allowing_an_unknown_code_allows_nothing() {
        let store = MemoryAuthStore::default();

        let allowed = store
            .allow(&ShortCode::random(), None)
            .await
            .expect("allowed");

        assert!(allowed.is_none());
        assert!(store.list_allowed().await.expect("listed").is_empty());
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

    #[tokio::test(start_paused = true)]
    async fn updating_rewrites_an_allowed_passkey() {
        let store = MemoryAuthStore::default();
        let mut allowed = allowed_passkey(&store).await;
        let used_at = Utc::now();
        allowed.last_used_at = Some(used_at);

        let updated = store
            .update_allowed(allowed.clone())
            .await
            .expect("updated");

        assert_eq!(updated, Update::Updated);
        let found = store
            .find_allowed(allowed.passkey.cred_id())
            .await
            .expect("found");
        assert_eq!(found.and_then(|a| a.last_used_at), Some(used_at));
    }

    #[tokio::test(start_paused = true)]
    async fn updating_a_revoked_passkey_leaves_it_revoked() {
        let store = MemoryAuthStore::default();
        let allowed = allowed_passkey(&store).await;
        store.remove_allowed(&allowed.code).await.expect("removed");

        let updated = store
            .update_allowed(allowed.clone())
            .await
            .expect("updated");

        assert_eq!(updated, Update::Gone);
        let found = store
            .find_allowed(allowed.passkey.cred_id())
            .await
            .expect("found");
        assert!(found.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn revoking_removes_only_the_passkey_with_that_code() {
        let store = MemoryAuthStore::default();
        let revoked = allowed_passkey(&store).await;
        let kept = allowed_passkey(&store).await;

        let removed = store.remove_allowed(&revoked.code).await.expect("removed");

        assert_eq!(removed.map(|r| r.code), Some(revoked.code));
        let gone = store
            .find_allowed(revoked.passkey.cred_id())
            .await
            .expect("found");
        assert!(gone.is_none());
        let listed = store.list_allowed().await.expect("listed");
        assert_eq!(
            listed.into_iter().map(|a| a.code).collect::<Vec<_>>(),
            vec![kept.code]
        );
    }
}
