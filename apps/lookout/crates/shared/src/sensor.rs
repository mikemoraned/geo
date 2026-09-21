//! The per-source sensor readings a device captures. They arrive at different rates
//! and are carried by their own message variants, so each is an independent payload.
//!
//! The GPS fix is [`domain::Gps`]: a fix means the same thing on a device, in a message and in
//! the store, so it is described where all three can reach it.

use serde::{Deserialize, Serialize};

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
