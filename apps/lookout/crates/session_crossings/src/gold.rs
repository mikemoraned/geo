use std::collections::HashMap;

use chrono::{DateTime, Utc};
use domain::SessionId;
use domain::{Gps, Sample};
use medallion::{Query, Root};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choosing {
    pub min_crossings: usize,
    pub max_sessions: usize,
}

const COORDINATE_PLACES: f64 = 1e6;
const READING_PLACES: f64 = 1e1;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Replay {
    pub session: SessionId,
    pub crossings: usize,
    pub samples: Vec<Sample<f64>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ChooseError {
    #[error("reading the silver datasets: {0}")]
    Query(#[from] medallion::QueryError),
    #[error("{0} has not been derived yet, so there is nothing to choose from")]
    Missing(&'static str),
}

pub async fn choose(root: &Root, choosing: Choosing) -> Result<Vec<Replay>, ChooseError> {
    let query = Query::new(root.clone());
    for (dataset, table) in [
        (medallion_model::SESSION_CROSSING, "session_crossing"),
        (medallion_model::SESSION_SAMPLE, "session_sample"),
    ] {
        if !query.register_if_present(dataset, table).await? {
            return Err(ChooseError::Missing(table));
        }
    }

    let counted: Vec<Counted> = query
        .rows(&format!(
            "SELECT session_id, COUNT(*) AS crossings FROM session_crossing
             GROUP BY session_id
             HAVING COUNT(*) >= {min}
             ORDER BY crossings DESC, session_id
             LIMIT {max}",
            min = choosing.min_crossings,
            max = choosing.max_sessions,
        ))
        .await?;
    if counted.is_empty() {
        return Ok(Vec::new());
    }

    let chosen: Vec<String> = counted
        .iter()
        .map(|session| format!("'{}'", session.session_id))
        .collect();
    let samples: Vec<StoredSample> = query
        .rows(&format!(
            "SELECT session_id, t, lat, lon, alt, acc, speed, heading
             FROM session_sample
             WHERE session_id IN ({})
             ORDER BY session_id, seq",
            chosen.join(", "),
        ))
        .await?;

    let mut by_session: HashMap<SessionId, Vec<Sample<f64>>> = HashMap::new();
    for sample in samples {
        by_session
            .entry(sample.session_id.clone())
            .or_default()
            .push(Sample {
                t: sample.t,
                gps: Gps::at(
                    round(sample.lat, COORDINATE_PLACES),
                    round(sample.lon, COORDINATE_PLACES),
                )
                .expect("on the globe")
                .with_altitude_metres(sample.alt.map(|alt| round(alt, READING_PLACES)))
                .with_accuracy_metres(Some(round(sample.acc, READING_PLACES)))
                .with_speed_mps(sample.speed.map(|speed| round(speed, READING_PLACES)))
                .with_heading_degrees(sample.heading.map(|to| round(to, READING_PLACES))),
            });
    }

    Ok(counted
        .into_iter()
        .map(|session| Replay {
            samples: by_session.remove(&session.session_id).unwrap_or_default(),
            crossings: session.crossings as usize,
            session: session.session_id,
        })
        .collect())
}

fn round(value: f64, places: f64) -> f64 {
    (value * places).round() / places
}

#[derive(Debug, Deserialize)]
struct Counted {
    session_id: SessionId,
    crossings: i64,
}

#[derive(Debug, Deserialize)]
struct StoredSample {
    session_id: SessionId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    t: DateTime<Utc>,
    lat: f64,
    lon: f64,
    alt: Option<f64>,
    acc: f64,
    speed: Option<f64>,
    heading: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_coordinate_is_kept_to_six_places() {
        assert_eq!(round(50.706_173_672_784_79, COORDINATE_PLACES), 50.706_174);
    }

    #[test]
    fn a_reading_is_kept_to_one() {
        assert_eq!(round(36.804_149_602_093_82, READING_PLACES), 36.8);
        assert_eq!(round(344.826_904_296_875, READING_PLACES), 344.8);
    }
}
