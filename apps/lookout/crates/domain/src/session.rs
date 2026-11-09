use chrono::{DateTime, Utc};
use nutype::nutype;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::device::DeviceId;
use crate::name::{NameError, checked};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartedBy {
    StartSession,
    Gap,
    FirstSeen,
}

#[nutype(
    validate(with = checked, error = NameError),
    derive(
        Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Display, FromStr, TryFrom, Into, AsRef,
        Serialize, Deserialize
    )
)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(id: impl Into<String>) -> Result<Self, NameError> {
        Self::try_new(id)
    }

    pub fn of(device: &DeviceId, started_at: DateTime<Utc>) -> Self {
        let namespace = Uuid::new_v5(&Uuid::nil(), b"SESSION_NAMESPACE");
        let name = format!("{device}/{}", started_at.timestamp_millis());
        Self::from(Uuid::new_v5(&namespace, name.as_bytes()))
    }
}

impl From<Uuid> for SessionId {
    fn from(id: Uuid) -> Self {
        Self::new(id.to_string()).expect("a UUID's text holds no reserved character")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> DeviceId {
        DeviceId::from(Uuid::from_u128(1))
    }

    fn instant() -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_700_000_000_000).expect("an instant")
    }

    #[test]
    fn an_id_is_the_same_for_the_same_device_and_start() {
        assert_eq!(
            SessionId::of(&device(), instant()),
            SessionId::of(&device(), instant())
        );
    }

    #[test]
    fn an_id_differs_by_device_and_by_start() {
        let other = DeviceId::from(Uuid::from_u128(2));

        assert_ne!(
            SessionId::of(&device(), instant()),
            SessionId::of(&other, instant())
        );
        assert_ne!(
            SessionId::of(&device(), instant()),
            SessionId::of(&device(), instant() + chrono::TimeDelta::seconds(1))
        );
    }

    #[test]
    fn an_id_is_the_one_the_store_was_written_with() {
        let device = DeviceId::new("77a64f88-c65f-4f9f-90bb-0d069b9f55a1").expect("an id");
        let started_at = DateTime::from_timestamp_millis(1_785_493_558_944).expect("an instant");

        assert_eq!(
            SessionId::of(&device, started_at).to_string(),
            "577c2fa4-3d37-5e83-ae8b-1c72a9ed46bf"
        );
    }

    #[test]
    fn a_derived_id_can_be_written_as_a_name() {
        let id = SessionId::of(&device(), instant());

        assert_eq!(SessionId::new(id.to_string()).expect("a name"), id);
    }

    #[test]
    fn an_id_that_could_not_be_written_as_a_name_is_refused() {
        assert!(SessionId::new("a session/one").is_err());
        assert!("".parse::<SessionId>().is_err());
    }

    #[test]
    fn an_id_is_checked_when_it_is_read_back() {
        assert!(serde_json::from_str::<SessionId>(r#""a-session""#).is_ok());
        assert!(serde_json::from_str::<SessionId>(r#""a session/one""#).is_err());
    }
}
