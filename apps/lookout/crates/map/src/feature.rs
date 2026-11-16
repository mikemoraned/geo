use std::io::{self, Write};

use geojson::Geometry;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum FeatureError {
    #[error("writing a feature: {0}")]
    Io(#[from] io::Error),
    #[error("encoding a feature: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename = "Feature")]
pub struct Feature<P> {
    pub tippecanoe: Tippecanoe,
    pub geometry: Geometry,
    pub properties: P,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Tippecanoe {
    pub layer: &'static str,
    pub minzoom: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maxzoom: Option<u8>,
}

impl<P: Serialize> Feature<P> {
    pub fn write_line(&self, mut to: impl Write) -> Result<(), FeatureError> {
        serde_json::to_writer(&mut to, self)?;
        to.write_all(b"\n")?;
        Ok(())
    }
}
