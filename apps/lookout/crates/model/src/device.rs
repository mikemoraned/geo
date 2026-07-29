//! The identity a device carries through the store.

use std::fmt::{self, Display};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Which device a row came from.
///
/// Stored as the string it reads as, since it joins across datasets by value and every
/// engine reading the store compares strings the same way. Devices mint their own ids, so
/// the store holds whatever a device sent rather than a shape it must conform to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(String);

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
        assert_eq!(
            "device-a".parse::<DeviceId>().unwrap().to_string(),
            "device-a"
        );
    }

    /// An empty id joins every row of every device together, so it is not an id.
    #[test]
    fn an_empty_id_is_rejected() {
        assert!(DeviceId::new("").is_err());
    }
}
