use std::path::PathBuf;
use geo::Coord;
use serde::Deserialize;
use overturemaps::overturemaps::GersId;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub bounds: Bounds,
    pub overturemaps: Option<OvertureMaps>,
}

#[derive(Deserialize, Debug)]
pub struct OvertureMaps {
    pub gers_id: GersId,
}

#[derive(Deserialize, Debug)]
pub struct Bounds {
    pub point1: Coord,
    pub point2: Coord,
    pub name: String
}

impl Config {
    pub fn read_from_file(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let config_str = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&config_str)?)
    }
}