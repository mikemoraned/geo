//! Reading the cell's voltage. What the voltage *means* is the core's, in
//! [`platform_core::battery`], where a whole discharge runs as a unit test.
//!
//! The numbers come from M5's own board table for `board_M5StickCPlus2`
//! (`M5Unified/src/utility/Power_Class.cpp`), which is also where G4 is confirmed as this
//! board's hold pin — a useful sign the right board is being read. There is no PMIC to ask:
//! the PLUS2 has no AXP192.

use esp_idf_svc::hal::adc::{
    attenuation,
    oneshot::config::{AdcChannelConfig, Calibration},
};

/// The cell is read through a divider that halves it, so the pin sees half the terminal
/// voltage.
const DIVIDER: f32 = 2.0;

/// How the ADC channel wants configuring: 12dB attenuation over a 12-bit conversion.
///
/// Calibration turns a raw count into millivolts at the pin using the constants burned into
/// this chip's eFuse, so a reading does not depend on its ADC being nominal. The original
/// ESP32 offers line fitting only — curve fitting is a C3/C6/S3 feature — which is the same
/// fallback M5's own code takes.
pub fn config() -> AdcChannelConfig {
    AdcChannelConfig {
        attenuation: attenuation::DB_12,
        calibration: Calibration::Line,
        ..Default::default()
    }
}

/// The terminal voltage, from what the pin measured.
pub fn terminal_millivolts(at_pin: u16) -> u16 {
    (f32::from(at_pin) * DIVIDER) as u16
}
