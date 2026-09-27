use std::fmt::{self, Display};
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::device::DeviceId;
use crate::name::{NameError, checked};

const SESSION_NAMESPACE: Uuid = Uuid::from_u128(0x8f9c_1d3a_6b47_4e21_9a05_c7d8_e2f4_1b60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartedBy {
    StartSession,
    Gap,
    FirstSeen,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(id: impl Into<String>) -> Result<Self, NameError> {
        Ok(Self(checked(id.into())?))
    }

    pub fn of(device: &DeviceId, started_at: DateTime<Utc>) -> Self {
        let name = format!("{device}/{}", started_at.timestamp_millis());
        Self(Uuid::new_v5(&SESSION_NAMESPACE, name.as_bytes()).to_string())
    }
}

impl FromStr for SessionId {
    type Err = NameError;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::new(id)
    }
}

impl Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
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
            "aab6af04-e435-5d49-8faa-d4cc8c999a05"
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
}
