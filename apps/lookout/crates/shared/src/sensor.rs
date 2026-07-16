//! The per-source sensor readings a device captures. They arrive at different rates
//! and are carried by their own message variants, so each is an independent payload.

use serde::{Deserialize, Serialize};

/// A GPS fix. `alt` is optional because `Geolocation` may report a null altitude.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gps {
    pub lat: f64,
    pub lon: f64,
    pub alt: Option<f64>,
    /// Accuracy of the fix, in metres.
    pub acc: f64,
}

/// An accelerometer reading. Each axis is optional because `DeviceMotionEvent`
/// may report null components on devices without a full accelerometer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Accel {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
}
