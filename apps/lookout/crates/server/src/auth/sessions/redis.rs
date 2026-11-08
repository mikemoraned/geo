use async_trait::async_trait;
use redis::aio::MultiplexedConnection;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{self, SessionStore};

use crate::auth::fresh::{Claim, ClaimError, claim_fresh};

fn session_key(id: &Id) -> String {
    format!("lookout-session:{id}")
}

fn backend(err: redis::RedisError) -> session_store::Error {
    session_store::Error::Backend(err.to_string())
}

fn set(record: &Record) -> session_store::Result<redis::Cmd> {
    let json = serde_json::to_string(record)
        .map_err(|err| session_store::Error::Encode(err.to_string()))?;
    let mut cmd = redis::cmd("SET");
    cmd.arg(session_key(&record.id))
        .arg(json)
        .arg("EXAT")
        .arg(record.expiry_date.unix_timestamp());
    Ok(cmd)
}

#[derive(Debug, Clone)]
pub struct RedisSessionStore {
    conn: MultiplexedConnection,
}

impl RedisSessionStore {
    pub fn new(conn: MultiplexedConnection) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl SessionStore for RedisSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        let template = record.clone();
        record.id = claim_fresh(Id::default, |id| {
            let candidate = Record {
                id,
                ..template.clone()
            };
            let mut conn = self.conn.clone();
            async move {
                let created: Option<String> = set(&candidate)?
                    .arg("NX")
                    .query_async(&mut conn)
                    .await
                    .map_err(backend)?;
                Ok(match created {
                    Some(_) => Claim::Claimed,
                    None => Claim::Taken,
                })
            }
        })
        .await
        .map_err(|err| match err {
            ClaimError::Store(err) => err,
            none_free => session_store::Error::Backend(none_free.to_string()),
        })?;
        Ok(())
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let () = set(record)?
            .query_async(&mut self.conn.clone())
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn load(&self, id: &Id) -> session_store::Result<Option<Record>> {
        let json: Option<String> = redis::cmd("GET")
            .arg(session_key(id))
            .query_async(&mut self.conn.clone())
            .await
            .map_err(backend)?;
        json.map(|json| serde_json::from_str(&json))
            .transpose()
            .map_err(|err| session_store::Error::Decode(err.to_string()))
    }

    async fn delete(&self, id: &Id) -> session_store::Result<()> {
        let _: i64 = redis::cmd("DEL")
            .arg(session_key(id))
            .query_async(&mut self.conn.clone())
            .await
            .map_err(backend)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::ops::RangeInclusive;

    use tower_sessions::cookie::time::OffsetDateTime;

    use super::*;
    use crate::auth::sessions::{IDLE_LIFETIME, SIGNED_IN_PASSKEY};
    use test_redis::TestRedis;

    struct Running {
        redis: TestRedis,
        store: RedisSessionStore,
    }

    async fn running() -> Running {
        let redis = TestRedis::start().await;
        Running {
            store: RedisSessionStore::new(redis.connection()),
            redis,
        }
    }

    fn signed_in() -> Record {
        Record {
            id: Id::default(),
            data: [(SIGNED_IN_PASSKEY.to_string(), "a passkey".into())].into(),
            expiry_date: OffsetDateTime::now_utc() + IDLE_LIFETIME,
        }
    }

    fn within_a_minute_of_the_idle_lifetime() -> RangeInclusive<i64> {
        let lifetime = IDLE_LIFETIME.whole_seconds();
        lifetime - 60..=lifetime
    }

    #[tokio::test]
    async fn a_created_session_loads_back_docker() {
        let running = running().await;
        let mut record = signed_in();

        running.store.create(&mut record).await.expect("created");

        let loaded = running.store.load(&record.id).await.expect("loaded");
        assert_eq!(loaded, Some(record));
    }

    #[tokio::test]
    async fn creating_over_a_held_id_draws_another_docker() {
        let running = running().await;
        let held = signed_in();
        running.store.save(&held).await.expect("saved");
        let mut colliding = Record {
            id: held.id,
            ..signed_in()
        };

        running.store.create(&mut colliding).await.expect("created");

        assert_ne!(colliding.id, held.id);
        let kept = running.store.load(&held.id).await.expect("loaded");
        assert_eq!(kept, Some(held));
        let created = running.store.load(&colliding.id).await.expect("loaded");
        assert_eq!(created, Some(colliding));
    }

    #[tokio::test]
    async fn saving_replaces_a_session_docker() {
        let running = running().await;
        let mut record = signed_in();
        running.store.create(&mut record).await.expect("created");
        record.data.clear();

        running.store.save(&record).await.expect("saved");

        let loaded = running.store.load(&record.id).await.expect("loaded");
        assert_eq!(loaded, Some(record));
    }

    #[tokio::test]
    async fn a_deleted_session_loads_nothing_docker() {
        let running = running().await;
        let mut record = signed_in();
        running.store.create(&mut record).await.expect("created");

        running.store.delete(&record.id).await.expect("deleted");

        let loaded = running.store.load(&record.id).await.expect("loaded");
        assert_eq!(loaded, None);
    }

    #[tokio::test]
    async fn an_unknown_session_loads_nothing_docker() {
        let running = running().await;

        let loaded = running.store.load(&Id::default()).await.expect("loaded");

        assert_eq!(loaded, None);
    }

    #[tokio::test]
    async fn a_session_expires_with_its_record_docker() {
        let running = running().await;
        let mut record = signed_in();
        running.store.create(&mut record).await.expect("created");

        let ttl = running
            .redis
            .seconds_to_live(&session_key(&record.id))
            .await;

        assert!(
            within_a_minute_of_the_idle_lifetime().contains(&ttl),
            "{ttl}"
        );
    }
}
