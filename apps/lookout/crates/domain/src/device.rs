//! A device: what it is, what it says about itself, and the id it carries from the fix it
//! reports to the row that keeps it.

use std::fmt::{self, Display};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Which device a reading came from.
///
/// Held as the string it reads as, since it joins by value and every engine comparing two of
/// them compares strings. Devices mint their own, so whoever holds one holds whatever the
/// device sent rather than a shape it must conform to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(String);

/// Broad device class, classified client-side from what a browser exposes. Enough to tell
/// an iPhone, an iPad and a laptop apart when interpreting a session.
///
/// Stored as the name it reads as, which is what the derive writes and what every engine
/// reading it back compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    Iphone,
    Ipad,
    Laptop,
    #[default]
    Unknown,
}

/// What a device says about itself when recording starts.
///
/// The raw signals are kept beside the classification so a misclassification can be
/// re-derived from source rather than being the only record of what was seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_type: DeviceType,
    /// `navigator.platform` (e.g. "iPhone", "MacIntel").
    pub platform: String,
    /// `navigator.userAgent`.
    pub user_agent: String,
    /// UA-CH platform (e.g. "iOS", "macOS"), when exposed.
    pub os: Option<String>,
    pub os_version: Option<String>,
}

/// An id that identifies nothing.
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
