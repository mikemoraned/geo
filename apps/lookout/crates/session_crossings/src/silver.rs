//! Deriving the silver `session_crossing` dataset: which crossings each session passed.
//!
//! Both inputs are read a country at a time, because a distance is only a distance within one
//! projected zone and the zone is chosen per country. The output carries no geometry — a match
//! is a session, a crossing and an instant — so it is partitioned by the date it happened and
//! by nothing else.
//!
//! A run derives the whole dataset from the whole of silver, and replaces what it produces, so
//! a partition it no longer produces rows for goes with it.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use geo_types::{Point, Rect};
use medallion::{Country, Query, Replaced, Root, COUNTRY};
use model::{Bbox, CrossingId, DeviceId, SessionCrossingRow, SessionId};
use serde::Deserialize;

use crate::matching::{passes, Crossing, Radius, Sample, Session};

/// What one run derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MatchOutcome {
    /// Sessions read, over every country.
    pub sessions: usize,
    /// Crossings read.
    pub crossings: usize,
    /// Sessions that passed at least one crossing.
    pub sessions_matched: usize,
    /// Rows written: one per session and crossing passed.
    pub passes: usize,
    pub partitions: Replaced,
}

/// A failure deriving the crossings a session passed.
#[derive(Debug, thiserror::Error)]
pub enum CrossingError {
    #[error("reading the silver datasets: {0}")]
    Query(#[from] medallion::QueryError),
    #[error("{dataset} has not been derived yet, so there is nothing to match against")]
    Missing { dataset: &'static str },
    #[error("writing the dataset: {0}")]
    Replace(#[from] medallion::ReplaceError),
    #[error("building the rows: {0}")]
    Rows(#[from] medallion::RowError),
}

/// One session as the store holds it: its identity and the envelope of its path.
#[derive(Debug, Deserialize)]
struct StoredSession {
    session_id: SessionId,
    device_id: DeviceId,
    bbox: Bbox,
}

/// One sample as the store holds it, with its position taken out of the projected geometry
/// as plain numbers — this needs coordinates in metres, not a geometry to decode.
#[derive(Debug, Deserialize)]
struct StoredSample {
    session_id: SessionId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    t: DateTime<Utc>,
    x: f64,
    y: f64,
}

/// One crossing as the store holds it: in metres for the distance, in lat/lon for the prune.
#[derive(Debug, Deserialize)]
struct StoredCrossing {
    crossing_id: CrossingId,
    x: f64,
    y: f64,
    lon: f64,
    lat: f64,
}

/// Derive the crossings every session passed, and write them.
///
/// A country the store holds no sessions or no crossings for contributes nothing rather than
/// failing: a store can legitimately hold sessions in a country no extract has covered yet.
pub async fn derive(
    root: &Root,
    radius: Radius,
) -> Result<MatchOutcome, CrossingError> {
    let query = Query::new(root.clone());
    if !query.register_if_present(model::SESSION, "session").await? {
        return Err(CrossingError::Missing {
            dataset: model::SESSION.name,
        });
    }
    if !query
        .register_if_present(model::SESSION_SAMPLE, "session_sample")
        .await?
    {
        return Err(CrossingError::Missing {
            dataset: model::SESSION_SAMPLE.name,
        });
    }
    if !query
        .register_if_present(model::WATER_CROSSING, "water_crossing")
        .await?
    {
        return Err(CrossingError::Missing {
            dataset: model::WATER_CROSSING.name,
        });
    }

    let mut outcome = MatchOutcome::default();
    let mut passed: Vec<SessionCrossingRow> = Vec::new();
    for country in Country::ALL {
        let sessions = sessions_in(&query, country).await?;
        let crossings = crossings_in(&query, country).await?;
        outcome.sessions += sessions.len();
        outcome.crossings += crossings.len();

        let country_passes = passes(&sessions, &crossings, radius);
        outcome.sessions_matched += country_passes
            .iter()
            .map(|pass| pass.session_id.to_string())
            .collect::<std::collections::HashSet<_>>()
            .len();
        passed.extend(country_passes);
    }

    passed.sort_by_key(|pass| (pass.crossed_at, pass.crossing_id.to_string()));
    outcome.passes = passed.len();
    outcome.partitions = write(root, &passed).await?;
    Ok(outcome)
}

/// Every session of one country, with its samples in metres.
async fn sessions_in(query: &Query, country: Country) -> Result<Vec<Session>, CrossingError> {
    let stored: Vec<StoredSession> = query
        .rows(&format!(
            "SELECT session_id, device_id, bbox FROM session
             WHERE {COUNTRY} = '{country}'"
        ))
        .await?;
    let samples: Vec<StoredSample> = query
        .rows(&format!(
            "SELECT session_id, t,
                    ST_X(geometry_projected) AS x, ST_Y(geometry_projected) AS y
             FROM session_sample
             WHERE {COUNTRY} = '{country}'
             ORDER BY t"
        ))
        .await?;

    let mut by_session: HashMap<String, Vec<Sample>> = HashMap::new();
    for sample in samples {
        by_session
            .entry(sample.session_id.to_string())
            .or_default()
            .push(Sample {
                t: sample.t,
                at: Point::new(sample.x, sample.y),
            });
    }

    Ok(stored
        .into_iter()
        .map(|session| {
            let samples = by_session
                .remove(&session.session_id.to_string())
                .unwrap_or_default();
            Session {
                session_id: session.session_id,
                device_id: session.device_id,
                envelope: envelope(&session.bbox),
                samples,
            }
        })
        .collect())
}

/// Every crossing of one country.
async fn crossings_in(query: &Query, country: Country) -> Result<Vec<Crossing>, CrossingError> {
    let stored: Vec<StoredCrossing> = query
        .rows(&format!(
            "SELECT crossing_id,
                    ST_X(geometry_projected) AS x, ST_Y(geometry_projected) AS y,
                    ST_X(geometry) AS lon, ST_Y(geometry) AS lat
             FROM water_crossing
             WHERE {COUNTRY} = '{country}'"
        ))
        .await?;

    Ok(stored
        .into_iter()
        .map(|crossing| Crossing {
            crossing_id: crossing.crossing_id,
            at: Point::new(crossing.x, crossing.y),
            lat_lon: Point::new(crossing.lon, crossing.lat),
        })
        .collect())
}

/// The stored envelope as a rectangle to prune against.
fn envelope(bbox: &Bbox) -> Rect<f64> {
    Rect::new((bbox.xmin, bbox.ymin), (bbox.xmax, bbox.ymax))
}

/// Write the passes, one partition per date they happened on.
///
/// `passed` is ordered by instant, so each date's rows are one adjacent run.
async fn write(root: &Root, passed: &[SessionCrossingRow]) -> Result<Replaced, CrossingError> {
    let days = passed
        .chunk_by(|a, b| date_of(a) == date_of(b))
        .map(|day| Ok((date_of(&day[0]), medallion::batch(day)?)))
        .collect::<Result<Vec<_>, medallion::RowError>>()?;

    Ok(root
        .rows_of::<SessionCrossingRow>()
        .replace_dates(&days)
        .await?)
}

/// The date a pass is partitioned under: the date of the nearest sample.
fn date_of(pass: &SessionCrossingRow) -> NaiveDate {
    pass.crossed_at.date_naive()
}
