use async_trait::async_trait;
use redis::Script;
use redis::aio::MultiplexedConnection;
use serde::de::DeserializeOwned;
use webauthn_rs::prelude::CredentialID;

use crate::auth::code::ShortCode;
use crate::auth::store::{
    AllowedPasskey, AuthStore, CEREMONY_LIFETIME, Ceremony, CeremonyId, Insertion,
    PENDING_LIFETIME, PendingPasskey, StoreError, Update,
};

const PASSKEYS_KEY: &str = "lookout-passkeys";

const REPLACE_IF_PRESENT: &str = r"
if redis.call('HEXISTS', KEYS[1], ARGV[1]) == 1 then
  redis.call('HSET', KEYS[1], ARGV[1], ARGV[2])
  return 1
end
return 0
";

fn ceremony_key(id: CeremonyId) -> String {
    format!("lookout-ceremony:{id}")
}

fn pending_key(code: &ShortCode) -> String {
    format!("lookout-passkey-pending:{}", code.as_str())
}

fn decoded<T: DeserializeOwned>(record: Option<String>) -> Result<Option<T>, StoreError> {
    Ok(record.map(|json| serde_json::from_str(&json)).transpose()?)
}

pub struct RedisAuthStore {
    conn: MultiplexedConnection,
    replace_if_present: Script,
}

impl RedisAuthStore {
    pub fn new(conn: MultiplexedConnection) -> Self {
        Self {
            conn,
            replace_if_present: Script::new(REPLACE_IF_PRESENT),
        }
    }

    async fn take(&self, key: String) -> Result<Option<String>, StoreError> {
        Ok(redis::cmd("GETDEL")
            .arg(key)
            .query_async(&mut self.conn.clone())
            .await?)
    }
}

#[async_trait]
impl AuthStore for RedisAuthStore {
    async fn put_ceremony(&self, id: CeremonyId, ceremony: Ceremony) -> Result<(), StoreError> {
        let () = redis::cmd("SET")
            .arg(ceremony_key(id))
            .arg(serde_json::to_string(&ceremony)?)
            .arg("EX")
            .arg(CEREMONY_LIFETIME.as_secs())
            .query_async(&mut self.conn.clone())
            .await?;
        Ok(())
    }

    async fn take_ceremony(&self, id: CeremonyId) -> Result<Option<Ceremony>, StoreError> {
        decoded(self.take(ceremony_key(id)).await?)
    }

    async fn insert_pending(
        &self,
        code: &ShortCode,
        passkey: PendingPasskey,
    ) -> Result<Insertion, StoreError> {
        let set: Option<String> = redis::cmd("SET")
            .arg(pending_key(code))
            .arg(serde_json::to_string(&passkey)?)
            .arg("NX")
            .arg("EX")
            .arg(PENDING_LIFETIME.as_secs())
            .query_async(&mut self.conn.clone())
            .await?;
        Ok(match set {
            Some(_) => Insertion::Inserted,
            None => Insertion::CodeTaken,
        })
    }

    async fn take_pending(&self, code: &ShortCode) -> Result<Option<PendingPasskey>, StoreError> {
        decoded(self.take(pending_key(code)).await?)
    }

    async fn find_allowed(&self, id: &CredentialID) -> Result<Option<AllowedPasskey>, StoreError> {
        let record: Option<String> = redis::cmd("HGET")
            .arg(PASSKEYS_KEY)
            .arg(id.as_ref())
            .query_async(&mut self.conn.clone())
            .await?;
        decoded(record)
    }

    async fn is_allowed(&self, id: &CredentialID) -> Result<bool, StoreError> {
        Ok(redis::cmd("HEXISTS")
            .arg(PASSKEYS_KEY)
            .arg(id.as_ref())
            .query_async(&mut self.conn.clone())
            .await?)
    }

    async fn put_allowed(&self, passkey: AllowedPasskey) -> Result<(), StoreError> {
        let _: i64 = redis::cmd("HSET")
            .arg(PASSKEYS_KEY)
            .arg(passkey.passkey.cred_id().as_ref())
            .arg(serde_json::to_string(&passkey)?)
            .query_async(&mut self.conn.clone())
            .await?;
        Ok(())
    }

    async fn update_allowed(&self, passkey: AllowedPasskey) -> Result<Update, StoreError> {
        let replaced: i64 = self
            .replace_if_present
            .key(PASSKEYS_KEY)
            .arg(passkey.passkey.cred_id().as_ref())
            .arg(serde_json::to_string(&passkey)?)
            .invoke_async(&mut self.conn.clone())
            .await?;
        Ok(match replaced {
            1 => Update::Updated,
            _ => Update::Gone,
        })
    }

    async fn list_allowed(&self) -> Result<Vec<AllowedPasskey>, StoreError> {
        let records: Vec<String> = redis::cmd("HVALS")
            .arg(PASSKEYS_KEY)
            .query_async(&mut self.conn.clone())
            .await?;
        Ok(records
            .iter()
            .map(|json| serde_json::from_str(json))
            .collect::<Result<_, _>>()?)
    }

    async fn remove_allowed(&self, code: &ShortCode) -> Result<Option<AllowedPasskey>, StoreError> {
        let Some(allowed) = self
            .list_allowed()
            .await?
            .into_iter()
            .find(|allowed| &allowed.code == code)
        else {
            return Ok(None);
        };
        let removed: i64 = redis::cmd("HDEL")
            .arg(PASSKEYS_KEY)
            .arg(allowed.passkey.cred_id().as_ref())
            .query_async(&mut self.conn.clone())
            .await?;
        Ok((removed == 1).then_some(allowed))
    }
}

#[cfg(test)]
mod tests {
    use std::ops::RangeInclusive;
    use std::time::Duration;

    use super::*;
    use crate::auth::contract::{self, allowed_passkey, pending};
    use crate::auth::test_passkeys::{registered_passkey, registration_ceremony};
    use test_redis::TestRedis;

    struct Running {
        redis: TestRedis,
        store: RedisAuthStore,
    }

    async fn running() -> Running {
        let redis = TestRedis::start().await;
        Running {
            store: RedisAuthStore::new(redis.connection()),
            redis,
        }
    }

    fn within_a_minute_of(lifetime: Duration) -> RangeInclusive<i64> {
        let lifetime = lifetime.as_secs() as i64;
        lifetime - 60..=lifetime
    }

    #[tokio::test]
    async fn a_ceremony_is_taken_once_docker() {
        contract::a_ceremony_is_taken_once(&running().await.store).await;
    }

    #[tokio::test]
    async fn a_pending_passkey_is_taken_once_by_its_code_docker() {
        contract::a_pending_passkey_is_taken_once_by_its_code(&running().await.store).await;
    }

    #[tokio::test]
    async fn a_held_code_refuses_another_pending_passkey_docker() {
        contract::a_held_code_refuses_another_pending_passkey(&running().await.store).await;
    }

    #[tokio::test]
    async fn allowing_moves_a_pending_passkey_into_the_allowed_ones_docker() {
        contract::allowing_moves_a_pending_passkey_into_the_allowed_ones(&running().await.store)
            .await;
    }

    #[tokio::test]
    async fn allowing_an_unknown_code_allows_nothing_docker() {
        contract::allowing_an_unknown_code_allows_nothing(&running().await.store).await;
    }

    #[tokio::test]
    async fn updating_rewrites_an_allowed_passkey_docker() {
        contract::updating_rewrites_an_allowed_passkey(&running().await.store).await;
    }

    #[tokio::test]
    async fn updating_a_revoked_passkey_leaves_it_revoked_docker() {
        contract::updating_a_revoked_passkey_leaves_it_revoked(&running().await.store).await;
    }

    #[tokio::test]
    async fn revoking_removes_only_the_passkey_with_that_code_docker() {
        contract::revoking_removes_only_the_passkey_with_that_code(&running().await.store).await;
    }

    #[tokio::test]
    async fn only_an_allowed_passkey_is_allowed_docker() {
        contract::only_an_allowed_passkey_is_allowed(&running().await.store).await;
    }

    #[tokio::test]
    async fn revoking_an_unknown_code_removes_nothing_docker() {
        contract::revoking_an_unknown_code_removes_nothing(&running().await.store).await;
    }

    #[tokio::test]
    async fn a_ceremony_expires_after_ten_minutes_docker() {
        let running = running().await;
        let id = CeremonyId::random();
        running
            .store
            .put_ceremony(id, registration_ceremony())
            .await
            .expect("stored");

        let ttl = running
            .redis
            .seconds_to_live(&format!("lookout-ceremony:{id}"))
            .await;

        assert!(
            within_a_minute_of(CEREMONY_LIFETIME).contains(&ttl),
            "{ttl}"
        );
    }

    #[tokio::test]
    async fn a_pending_passkey_expires_after_an_hour_docker() {
        let running = running().await;
        let code = running
            .store
            .put_pending(pending(registered_passkey().await))
            .await
            .expect("stored");

        let ttl = running
            .redis
            .seconds_to_live(&format!("lookout-passkey-pending:{}", code.as_str()))
            .await;

        assert!(within_a_minute_of(PENDING_LIFETIME).contains(&ttl), "{ttl}");
    }

    #[tokio::test]
    async fn the_allowed_passkeys_never_expire_docker() {
        let running = running().await;
        allowed_passkey(&running.store).await;

        let ttl = running.redis.seconds_to_live("lookout-passkeys").await;

        assert_eq!(ttl, -1);
    }
}
