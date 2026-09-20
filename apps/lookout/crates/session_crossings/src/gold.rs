//! Choosing the recorded sessions worth replaying, and writing them where a page can fetch
//! them.
//!
//! Worth replaying means passing crossings, since a session that passes none shows an empty
//! screen for as long as it runs. Which sessions those are is already derived:
//! `session_crossing` holds a row per session per crossing passed, so choosing is a count over
//! it rather than a second pass over the geometry.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use medallion::{Query, Root};
use medallion_model::SessionId;
use model::Gps;
use serde::{Deserialize, Serialize};

/// How a session is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choosing {
    /// How many crossings a session must pass to be worth replaying at all.
    pub min_crossings: usize,
    /// How many of the best to keep.
    pub max_sessions: usize,
}

/// Decimal places kept for a coordinate, worth about 11cm of latitude, and for everything
/// else, worth a tenth of a metre or a tenth of a metre per second.
///
/// A position comes out of the store as `f64` and writing one takes seventeen significant
/// digits, which is nanometres. The fix it came from was accurate to tens of metres.
const COORDINATE_PLACES: f64 = 1e6;
const READING_PLACES: f64 = 1e1;

/// One session, as a page replays it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Replay {
    pub session: SessionId,
    /// How many crossings it passed, which is why it was chosen.
    pub crossings: usize,
    pub samples: Vec<Fix>,
}

/// One sample, shaped as the event a shell sends: a page replays a session by sending these in
/// order, and needs to reshape nothing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Fix {
    pub t: DateTime<Utc>,
    pub gps: Gps,
}

/// A failure choosing what to replay.
#[derive(Debug, thiserror::Error)]
pub enum ChooseError {
    #[error("reading the silver datasets: {0}")]
    Query(#[from] medallion::QueryError),
    #[error("{0} has not been derived yet, so there is nothing to choose from")]
    Missing(&'static str),
}

/// The sessions worth replaying, most crossings first, with the samples that replay them.
///
/// # Errors
///
/// Returns an error where a dataset it reads has not been derived, or a query fails.
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

    // Read in one query and grouped here, rather than a query per session: the chosen few are
    // a handful and the samples are one scan either way.
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

    let mut by_session: HashMap<SessionId, Vec<Fix>> = HashMap::new();
    for sample in samples {
        by_session
            .entry(sample.session_id.clone())
            .or_default()
            .push(Fix {
                t: sample.t,
                gps: Gps {
                    latitude: round(sample.lat, COORDINATE_PLACES),
                    longitude: round(sample.lon, COORDINATE_PLACES),
                    altitude_metres: sample.alt.map(|alt| round(alt, READING_PLACES)),
                    accuracy_metres: round(sample.acc, READING_PLACES),
                    speed_mps: sample.speed.map(|speed| round(speed, READING_PLACES)),
                    heading_degrees: sample.heading.map(|to| round(to, READING_PLACES)),
                },
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

/// A value at the precision it is written out in.
fn round(value: f64, places: f64) -> f64 {
    (value * places).round() / places
}

/// One session and how many crossings it passed.
#[derive(Debug, Deserialize)]
struct Counted {
    session_id: SessionId,
    crossings: i64,
}

/// One sample as the store holds it, in the terms a shell reports one in.
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

    /// Eleven centimetres, against a fix accurate to tens of metres.
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
