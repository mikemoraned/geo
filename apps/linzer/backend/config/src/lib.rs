use overturemaps::overturemaps::GersId;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub overturemaps: OvertureMaps,
}

#[derive(Deserialize, Debug)]
pub struct OvertureMaps {
    pub gers_id: GersId,
}

impl Config {
    pub fn read_from_file(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let config_str = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&config_str)?)
    }
}
