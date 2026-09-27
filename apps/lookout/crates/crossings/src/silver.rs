use domain::{CoordinateError, CrossingCompactId, CrossingId};
use medallion::{Query, Root};
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
struct StoredCrossing {
    crossing_id: CrossingId,
    crossing_compact_id: CrossingCompactId,
    extract_id: String,
    lon: f64,
    lat: f64,
}

pub async fn read(root: &Root) -> Result<Vec<Crossing>, ReadError> {
    let query = Query::new(root.clone());
    if !query
        .register_if_present(medallion_model::WATER_CROSSING, "water_crossing")
        .await?
    {
        return Err(ReadError::Missing {
            dataset: medallion_model::WATER_CROSSING.name,
        });
    }

    let stored: Vec<StoredCrossing> = query
        .rows(
            "SELECT crossing_id, crossing_compact_id, extract_id,
                    ST_X(geometry) AS lon, ST_Y(geometry) AS lat
             FROM water_crossing",
        )
        .await?;

    stored
        .into_iter()
        .map(|crossing| {
            Ok(Crossing {
                crossing: domain::Crossing::at(crossing.crossing_id, crossing.lat, crossing.lon)?,
                compact_id: crossing.crossing_compact_id,
                extract_id: crossing.extract_id,
            })
        })
        .collect()
}
