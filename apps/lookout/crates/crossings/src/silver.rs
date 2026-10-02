use domain::{CoordinateError, CrossingCompactId};
use medallion::Root;
use medallion_model::WaterCrossingRow;
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("reading the store: {0}")]
    Query(#[from] medallion::QueryError),
    #[error("{dataset} has not been derived yet, so there is nothing to pack")]
    Missing { dataset: &'static str },
    #[error("the store holds a crossing that is not on the globe: {0}")]
    OffTheGlobe(#[from] CoordinateError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Crossing {
    pub crossing: domain::Crossing,
    pub compact_id: CrossingCompactId,
    pub extract_id: String,
}

#[derive(Debug, Deserialize)]
pub struct PlacedCrossing {
    #[serde(flatten)]
    pub row: WaterCrossingRow,
    pub lon: f64,
    pub lat: f64,
}

pub const PLACED: &str = "SELECT *, ST_X(geometry) AS lon, ST_Y(geometry) AS lat
                          FROM water_crossing";

pub async fn read(root: &Root) -> Result<Vec<Crossing>, ReadError> {
    let stored: Vec<PlacedCrossing> = medallion::rows_of_every_country(
        root,
        medallion_model::WATER_CROSSING,
        "water_crossing",
        PLACED,
    )
    .await?;
    if stored.is_empty() {
        return Err(ReadError::Missing {
            dataset: medallion_model::WATER_CROSSING.name,
        });
    }

    stored
        .into_iter()
        .map(|crossing| {
            Ok(Crossing {
                crossing: domain::Crossing::at(
                    crossing.row.crossing_id,
                    crossing.lat,
                    crossing.lon,
                )?,
                compact_id: crossing.row.crossing_compact_id,
                extract_id: crossing.row.extract_id,
            })
        })
        .collect()
}
