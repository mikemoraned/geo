use serde::{Deserialize, Serialize};

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
