#![cfg(test)]

use chrono::Utc;
use webauthn_rs::prelude::Passkey;

use crate::auth::code::ShortCode;
use crate::auth::store::{
    AllowedPasskey, AuthStore, CeremonyId, Insertion, PendingPasskey, Update,
};
use crate::auth::test_passkeys::{registered_passkey, registration_ceremony};

pub fn pending(passkey: Passkey) -> PendingPasskey {
    PendingPasskey {
        passkey,
        registered_at: Utc::now(),
    }
}

pub async fn allowed_passkey(store: &dyn AuthStore) -> AllowedPasskey {
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

pub async fn a_ceremony_is_taken_once(store: &dyn AuthStore) {
    let id = CeremonyId::random();
    store
        .put_ceremony(id, registration_ceremony())
        .await
        .expect("stored");

    assert!(store.take_ceremony(id).await.expect("taken").is_some());
    assert!(store.take_ceremony(id).await.expect("taken").is_none());
}

pub async fn a_pending_passkey_is_taken_once_by_its_code(store: &dyn AuthStore) {
    let passkey = registered_passkey().await;
    let id = passkey.cred_id().clone();
    let code = store.put_pending(pending(passkey)).await.expect("stored");

    let taken = store.take_pending(&code).await.expect("taken");
    assert_eq!(taken.map(|p| p.passkey.cred_id().clone()), Some(id));
    assert!(store.take_pending(&code).await.expect("taken").is_none());
}

pub async fn a_held_code_refuses_another_pending_passkey(store: &dyn AuthStore) {
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

pub async fn allowing_moves_a_pending_passkey_into_the_allowed_ones(store: &dyn AuthStore) {
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

pub async fn allowing_an_unknown_code_allows_nothing(store: &dyn AuthStore) {
    let allowed = store
        .allow(&ShortCode::random(), None)
        .await
        .expect("allowed");

    assert!(allowed.is_none());
    assert!(store.list_allowed().await.expect("listed").is_empty());
}

pub async fn updating_rewrites_an_allowed_passkey(store: &dyn AuthStore) {
    let mut allowed = allowed_passkey(store).await;
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

pub async fn updating_a_revoked_passkey_leaves_it_revoked(store: &dyn AuthStore) {
    let allowed = allowed_passkey(store).await;
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

pub async fn revoking_removes_only_the_passkey_with_that_code(store: &dyn AuthStore) {
    let revoked = allowed_passkey(store).await;
    let kept = allowed_passkey(store).await;

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

pub async fn revoking_an_unknown_code_removes_nothing(store: &dyn AuthStore) {
    let kept = allowed_passkey(store).await;

    let removed = store
        .remove_allowed(&ShortCode::random())
        .await
        .expect("removed");

    assert!(removed.is_none());
    let listed = store.list_allowed().await.expect("listed");
    assert_eq!(
        listed.into_iter().map(|a| a.code).collect::<Vec<_>>(),
        vec![kept.code]
    );
}
