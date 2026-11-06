use std::fmt;

pub const REDIS_URL_VAR: &str = "LOOKOUT_REDIS_URL";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, clap::ValueEnum)]
pub enum Store {
    Redis,
    InMemory,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Stores {
    InMemory,
    Redis { url: String },
}

impl fmt::Debug for Stores {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stores::InMemory => f.write_str("InMemory"),
            Stores::Redis { .. } => f.debug_struct("Redis").finish_non_exhaustive(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoresError {
    #[error("the redis store needs {REDIS_URL_VAR} to be set")]
    NoRedisUrl,
}

impl Stores {
    pub fn choose(store: Store, redis_url: Option<String>) -> Result<Self, StoresError> {
        match (store, redis_url) {
            (Store::Redis, Some(url)) => Ok(Stores::Redis { url }),
            (Store::Redis, None) => Err(StoresError::NoRedisUrl),
            (Store::InMemory, _) => Ok(Stores::InMemory),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "redis://localhost:6379";

    #[test]
    fn redis_takes_the_redis_url() {
        assert_eq!(
            Stores::choose(Store::Redis, Some(URL.into())),
            Ok(Stores::Redis { url: URL.into() })
        );
    }

    #[test]
    fn in_memory_needs_no_redis_url() {
        assert_eq!(Stores::choose(Store::InMemory, None), Ok(Stores::InMemory));
    }

    #[test]
    fn redis_without_a_url_is_refused() {
        assert_eq!(
            Stores::choose(Store::Redis, None),
            Err(StoresError::NoRedisUrl)
        );
    }

    #[test]
    fn in_memory_ignores_a_redis_url() {
        assert_eq!(
            Stores::choose(Store::InMemory, Some(URL.into())),
            Ok(Stores::InMemory)
        );
    }

    #[test]
    fn debugging_redis_hides_its_url() {
        let shown = format!("{:?}", Stores::Redis { url: URL.into() });

        assert!(!shown.contains(URL), "{shown}");
    }
}
