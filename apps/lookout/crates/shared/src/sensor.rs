//! The per-source sensor readings a device captures. They arrive at different rates
//! and are carried by their own message variants, so each is an independent payload.

use serde::{Deserialize, Serialize};

/// A GPS fix. `alt` is optional because `Geolocation` may report a null altitude.
///
/// `speed` (Doppler-derived, m/s) and `heading` (course over ground, degrees) come
/// straight from `coords` and are both nullable — `heading` is null when stationary,
/// and a null carries meaning, so the nulls are stored rather than dropped. They
/// default to `None` so historical v0 fixes (which lack them) still parse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gps {
    pub lat: f64,
    pub lon: f64,
    pub alt: Option<f64>,
    /// Accuracy of the fix, in metres.
    pub acc: f64,
    #[serde(default)]
    pub speed: Option<f64>,
    #[serde(default)]
    pub heading: Option<f64>,
}

/// An accelerometer reading aggregated over a sample window from the gravity-removed
/// `DeviceMotionEvent.acceleration`. At 0.1 Hz an instantaneous sample would just
/// measure gravity, so the window is reduced to `rms` (ride roughness), `peak` (jolts
/// / pointwork), and `n` (readings aggregated — confirms the window was sampled, not
/// suspended). A single raw instantaneous reading (`x`/`y`/`z`, gravity-removed) is
/// kept alongside for a tilt view. The aggregates default so historical v0 readings
/// (which carried only `x`/`y`/`z`) still parse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Accel {
    #[serde(default)]
    pub rms: f64,
    #[serde(default)]
    pub peak: f64,
    #[serde(default)]
    pub n: u32,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub z: Option<f64>,
}
