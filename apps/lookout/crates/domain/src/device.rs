use std::fmt::{self, Display};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
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

impl DeviceId {
    pub fn new(id: impl Into<String>) -> Result<Self, EmptyDeviceId> {
        let id = id.into();
        if id.is_empty() {
            return Err(EmptyDeviceId);
        }
        Ok(Self(id))
    }
}

impl From<Uuid> for DeviceId {
    fn from(id: Uuid) -> Self {
        Self(id.to_string())
    }
}

impl FromStr for DeviceId {
    type Err = EmptyDeviceId;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::new(id)
    }
}

impl Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
