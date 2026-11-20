use std::fmt::{self, Display};
use std::str::FromStr;

use hashfn::hashfn;
use serde::{Deserialize, Serialize, Serializer};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub struct DeviceId(Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    Iphone,
    Ipad,
    Laptop,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_type: DeviceType,
    pub platform: String,
    pub user_agent: String,
    pub os: Option<String>,
    pub os_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{id}` is not a device id: {source}")]
pub struct NotADeviceId {
    id: String,
    #[source]
    source: uuid::Error,
}

impl DeviceId {
    #[hashfn]
    pub fn new(id: impl Into<String>) -> Result<Self, NotADeviceId> {
        let id = id.into();
        match Uuid::parse_str(&id) {
            Ok(uuid) => Ok(Self(uuid)),
            Err(source) => Err(NotADeviceId { id, source }),
        }
    }
}

impl FromStr for DeviceId {
    type Err = NotADeviceId;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::new(id)
    }
}

impl TryFrom<String> for DeviceId {
    type Error = NotADeviceId;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        Self::new(id)
    }
}

impl From<Uuid> for DeviceId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl From<DeviceId> for String {
    fn from(id: DeviceId) -> Self {
        id.to_string()
    }
}

impl Serialize for DeviceId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct("DeviceId", &self.to_string())
    }
}

impl Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn text_that_is_not_a_uuid_is_rejected(text in "[^0-9a-fA-F-]{0,8}") {
            prop_assert!(DeviceId::new(text).is_err());
        }
    }

    #[test]
    fn an_id_reads_back_as_what_it_was_made_from() {
        let id = Uuid::from_u128(1);

        assert_eq!(
            DeviceId::from(id).to_string(),
            "00000000-0000-0000-0000-000000000001"
        );
    }

    #[test]
    fn an_id_is_parsed_from_a_uuids_text() {
        assert_eq!(
            "cdf2ab5a-0157-4dbe-8406-27a7d4c4970d"
                .parse::<DeviceId>()
                .unwrap(),
            DeviceId::from(Uuid::from_u128(0xcdf2ab5a_0157_4dbe_8406_27a7d4c4970d))
        );
    }

    #[test]
    fn an_empty_id_is_rejected() {
        assert!(DeviceId::new("").is_err());
        assert!("".parse::<DeviceId>().is_err());
    }

    #[test]
    fn an_id_is_checked_when_it_is_read_back() {
        assert!(
            serde_json::from_str::<DeviceId>(r#""cdf2ab5a-0157-4dbe-8406-27a7d4c4970d""#).is_ok()
        );
        assert!(serde_json::from_str::<DeviceId>(r#""device-a""#).is_err());
    }

    #[test]
    fn an_id_is_written_as_a_uuids_text() {
        assert_eq!(
            serde_json::to_string(&DeviceId::from(Uuid::from_u128(1))).unwrap(),
            r#""00000000-0000-0000-0000-000000000001""#
        );
    }
}
