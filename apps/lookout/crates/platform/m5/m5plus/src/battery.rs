use esp_idf_svc::hal::adc::{
    attenuation,
    oneshot::config::{AdcChannelConfig, Calibration},
};

const DIVIDER: f32 = 2.0;

pub fn config() -> AdcChannelConfig {
    AdcChannelConfig {
        attenuation: attenuation::DB_12,
        calibration: Calibration::Line,
        ..Default::default()
    }
}

pub fn terminal_millivolts(at_pin: u16) -> u16 {
    (f32::from(at_pin) * DIVIDER) as u16
}
