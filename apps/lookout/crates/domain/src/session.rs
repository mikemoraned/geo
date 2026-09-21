//! A session's identity: one contiguous run of samples from one device.

use std::fmt::{self, Display};
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::device::DeviceId;
use crate::name::{NameError, checked};

/// The namespace session ids are minted in, so an id derived here cannot collide with a
/// name-based id derived from the same values for anything else.
const SESSION_NAMESPACE: Uuid = Uuid::from_u128(0x8f9c_1d3a_6b47_4e21_9a05_c7d8_e2f4_1b60);

/// Identifies one session, on the session and on each of its samples.
///
/// Derived from what the session *is* rather than minted per run, so a run that re-derives a
/// session it has already written lands on the same id and rewrites it in place.
///
/// It is a name and stays writable as one: an id reaches a store that lays data out in
/// directories named by it, and a query that asks for one, so the characters those would
/// misread are refused here rather than where a path is built.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    /// An existing id.
    ///
    /// # Errors
    ///
    /// Returns an error where the id could not be written as a name.
    pub fn new(id: impl Into<String>) -> Result<Self, NameError> {
        Ok(Self(checked(id.into())?))
    }

    /// The id of the session `device` began at `started_at`.
    ///
    /// A name-based UUID over exactly what identifies the session, so any run — or any
    /// reader wanting to name a session it has only the boundaries of — arrives at the
    /// same id without consulting what has already been written.
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

    /// One session the store holds, and the id it was written under. Pinned rather than
    /// merely checked for determinism: changing the namespace or what is hashed would still
    /// derive consistently, and would rename every session already recorded.
    #[test]
    fn an_id_is_the_one_the_store_was_written_with() {
        let device = DeviceId::new("77a64f88-c65f-4f9f-90bb-0d069b9f55a1").expect("an id");
        let started_at = DateTime::from_timestamp_millis(1_785_493_558_944).expect("an instant");

        assert_eq!(
            SessionId::of(&device, started_at).to_string(),
            "aab6af04-e435-5d49-8faa-d4cc8c999a05"
        );
    }

    /// An id is written out as a directory name and asked for in a query, so a derived one
    /// has to be writable as a name.
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
