//! Read crossings out of the silver `water_crossing` dataset.
//!
//! Read through the store rather than off a file path, so what is packed is what every other
//! reader of the store sees, and so a crossing carries the extraction it was derived from.
//!
//! **Every country is read**, not one named by a caller: the buffer holds lat/lon and the
//! device's scan is a great-circle distance, so the per-country zone the dataset is
//! partitioned by never reaches it, and a device does not know which country it will be
//! switched on in. What a device can hold is a window, which is what a bbox restriction is
//! for.
//!
//! Position is taken from the geometry column rather than from any plain `lat`/`lon` columns,
//! because the geometry is where the dataset keeps it.

use geo_types::{Coord, coord};
use medallion::{Query, Root};
use model::CrossingId;
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("reading the store: {0}")]
    Query(#[from] medallion::QueryError),
    #[error("{dataset} has not been derived yet, so there is nothing to pack")]
    Missing { dataset: &'static str },
}

/// One crossing, as silver holds it: what it is called, what meets what, where along the
/// track, where on the globe, and which extraction it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Crossing {
    pub crossing_id: CrossingId,
    pub rail_id: String,
    pub water_id: String,
    /// How far along `rail_id` the crossing sits, from 0 at its start to 1 at its end.
    pub frac: f64,
    /// Longitude in `x`, latitude in `y`, in WGS84 degrees.
    pub position: Coord<f64>,
    /// The extraction the upstream reference rows came from.
    pub extract_id: String,
}

/// The columns as the query returns them, with the position as plain numbers — packing needs
/// two coordinates, not a geometry to decode.
#[derive(Debug, Deserialize)]
struct StoredCrossing {
    crossing_id: CrossingId,
    rail_id: String,
    water_id: String,
    frac: f64,
    extract_id: String,
    lon: f64,
    lat: f64,
}

/// Every crossing the store holds, in every country.
pub async fn read(root: &Root) -> Result<Vec<Crossing>, ReadError> {
    let query = Query::new(root.clone());
    if !query
        .register_if_present(model::WATER_CROSSING, "water_crossing")
        .await?
    {
        return Err(ReadError::Missing {
            dataset: model::WATER_CROSSING.name,
        });
    }

    let stored: Vec<StoredCrossing> = query
        .rows(
            "SELECT crossing_id, rail_id, water_id, frac, extract_id,
                    ST_X(geometry) AS lon, ST_Y(geometry) AS lat
             FROM water_crossing",
        )
        .await?;

    Ok(stored
        .into_iter()
        .map(|crossing| Crossing {
            crossing_id: crossing.crossing_id,
            rail_id: crossing.rail_id,
            water_id: crossing.water_id,
            frac: crossing.frac,
            position: coord! { x: crossing.lon, y: crossing.lat },
            extract_id: crossing.extract_id,
        })
        .collect())
}
