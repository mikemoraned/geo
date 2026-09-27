use std::fmt::{self, Display};
use std::str::FromStr;

use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GersId(Uuid);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{id}` is not a GERS id: {source}")]
pub struct NotAGersId {
    id: String,
    #[source]
    source: uuid::Error,
}

impl GersId {
    pub fn new(id: impl Into<String>) -> Result<Self, NotAGersId> {
        let id = id.into();
        match Uuid::parse_str(&id) {
            Ok(uuid) => Ok(Self(uuid)),
            Err(source) => Err(NotAGersId { id, source }),
        }
    }
}

impl FromStr for GersId {
    type Err = NotAGersId;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::new(id)
    }
}

impl Display for GersId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GERMANY: &str = "567d1698-7209-4b94-b7b8-0bc71bde0104";

    #[test]
    fn an_id_is_written_back_as_the_data_carries_it() {
        let id: GersId = GERMANY.parse().expect("a GERS id");

        assert_eq!(id.to_string(), GERMANY);
    }

    #[test]
    fn an_id_in_capitals_names_the_same_entity_as_one_in_lower_case() {
        let shouted = GersId::new(GERMANY.to_uppercase()).expect("a GERS id");

        assert_eq!(shouted, GersId::new(GERMANY).expect("a GERS id"));
        assert_eq!(shouted.to_string(), GERMANY);
    }

    #[test]
    fn a_string_that_is_no_uuid_is_not_a_gers_id() {
        assert!(GersId::new("").is_err());
        assert!(GersId::new("the-german-country-area").is_err());
        assert!(GersId::new(format!("{GERMANY}-0104")).is_err());
    }

    #[test]
    fn what_is_rejected_is_reported_with_the_id_that_was_offered() {
        let err = GersId::new("no-such-id").expect_err("no GERS id");

        assert!(err.to_string().contains("no-such-id"), "{err}");
    }
}
