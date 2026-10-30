use hashfn::hashfn;
use nutype::nutype;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[nutype(
    validate(with = device_id_present, error = EmptyDeviceId),
    derive(
        Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Display, FromStr, TryFrom, Into,
        Serialize, Deserialize
    )
)]
pub struct DeviceId(String);

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

#[derive(Debug, thiserror::Error)]
#[error("a device id cannot be empty")]
pub struct EmptyDeviceId;

#[hashfn]
pub fn device_id_present(id: &str) -> Result<(), EmptyDeviceId> {
    if id.is_empty() {
        Err(EmptyDeviceId)
    } else {
        Ok(())
    }
}

impl DeviceId {
    pub fn new(id: impl Into<String>) -> Result<Self, EmptyDeviceId> {
        Self::try_new(id)
    }
}

impl From<Uuid> for DeviceId {
    fn from(id: Uuid) -> Self {
        Self::new(id.to_string()).expect("a UUID's text is never empty")
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn an_id_is_accepted_exactly_when_it_is_not_empty(text in ".{0,8}") {
            prop_assert_eq!(DeviceId::new(text.clone()).is_ok(), !text.is_empty());
        }
    }

    #[test]
    fn an_id_reads_back_as_what_it_was_made_from() {
        let id = Uuid::from_u128(1);

        assert_eq!(DeviceId::from(id).to_string(), id.to_string());
    }

    #[test]
    fn an_empty_id_is_rejected() {
        assert!(DeviceId::new("").is_err());
        assert!("".parse::<DeviceId>().is_err());
    }

    #[test]
    fn an_id_is_checked_when_it_is_read_back() {
        assert!(serde_json::from_str::<DeviceId>(r#""device-a""#).is_ok());
        assert!(serde_json::from_str::<DeviceId>(r#""""#).is_err());
    }
}
