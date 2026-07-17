//! Session metadata: what a device announces about itself when recording starts.
//! It's captured once per session (a `StartSession` message) and interpreted into
//! the `device` table so per-sensor rows can join to their device's identity.

use serde::{Deserialize, Serialize};

/// Broad device class, classified client-side from what Safari exposes. Enough to
/// tell an iPhone / iPad / laptop apart when interpreting a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    Iphone,
    Ipad,
    Laptop,
    #[default]
    Unknown,
}

impl DeviceType {
    /// The wire/serde name, for storing in a text column.
    pub fn as_str(&self) -> &'static str {
        match self {
            DeviceType::Iphone => "iphone",
            DeviceType::Ipad => "ipad",
            DeviceType::Laptop => "laptop",
            DeviceType::Unknown => "unknown",
        }
    }
}

/// Metadata about the device a session runs on, derived from `navigator` and UA
/// client hints. The raw signals are kept alongside the classification so a
/// misclassification can be re-derived from source.
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
