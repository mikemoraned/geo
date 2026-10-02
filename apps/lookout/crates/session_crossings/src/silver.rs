use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use domain::Bbox;
use domain::{DeviceId, Pass, SessionId};
use geo_types::Point;
use medallion::{Country, DatasetSpec, Query, Replaced, Root, layers};
use medallion_model::{SessionCrossingRow, WaterCrossingRow};
use serde::Deserialize;

use crate::matching::{Crossing, Radius, Sample, Session, passes};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MatchOutcome {
    pub sessions: usize,
    pub crossings: usize,
    pub sessions_matched: usize,
    pub passes: usize,
    pub partitions: Replaced,
}

#[derive(Debug, thiserror::Error)]
pub enum CrossingError {
    #[error("reading the silver datasets: {0}")]
    Query(#[from] medallion::QueryError),
    #[error("{dataset} has not been derived yet, so there is nothing to match against")]
    Missing { dataset: &'static str },
    #[error("writing the dataset: {0}")]
    Write(#[from] medallion::TableError),
    #[error("the store holds a crossing that is not on the globe: {0}")]
    OffTheGlobe(#[from] domain::CoordinateError),
}

#[derive(Debug, Deserialize)]
struct StoredSession {
    session_id: SessionId,
    device_id: DeviceId,
    bbox: Bbox,
}

#[derive(Debug, Deserialize)]
struct StoredSample {
    session_id: SessionId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    t: DateTime<Utc>,
    x: f64,
    y: f64,
}

#[derive(Debug, Deserialize)]
struct PlacedCrossing {
    #[serde(flatten)]
    row: WaterCrossingRow,
    x: f64,
    y: f64,
    lon: f64,
    lat: f64,
}

pub async fn derive(root: &Root, radius: Radius) -> Result<MatchOutcome, CrossingError> {
    let mut outcome = MatchOutcome::default();
    let mut passed: Vec<SessionCrossingRow> = Vec::new();
    let mut derived_somewhere: HashSet<&'static str> = HashSet::new();

    for country in Country::ALL {
        let (query, held) = registered_for(root, country).await?;
        derived_somewhere.extend(held.iter());

        if held.len() == READ.len() {
            let sessions = sessions_in(&query).await?;
            let crossings = crossings_in(&query).await?;
            outcome.sessions += sessions.len();
            outcome.crossings += crossings.len();

            let country_passes = passes(&sessions, &crossings, radius);
            outcome.sessions_matched += country_passes
                .iter()
                .map(|pass| &pass.session_id)
                .collect::<HashSet<_>>()
                .len();
            let devices: HashMap<&SessionId, &DeviceId> = sessions
                .iter()
                .map(|session| (&session.session_id, &session.device_id))
                .collect();
            passed.extend(
                country_passes
                    .iter()
                    .map(|pass| row(pass, devices[&pass.session_id], radius)),
            );
        }
    }

    if let Some((dataset, _)) = READ
        .iter()
        .find(|(dataset, _)| !derived_somewhere.contains(dataset.name))
    {
        return Err(CrossingError::Missing {
            dataset: dataset.name,
        });
    }

    passed.sort_by(|a, b| (a.crossed_at, &a.crossing_id).cmp(&(b.crossed_at, &b.crossing_id)));
    outcome.passes = passed.len();
    outcome.partitions = medallion::write_rows(root, &passed).await?.partitions;
    Ok(outcome)
}

const READ: [(DatasetSpec<layers::Silver>, &str); 3] = [
    (medallion_model::SESSION, "session"),
    (medallion_model::SESSION_SAMPLE, "session_sample"),
    (medallion_model::WATER_CROSSING, "water_crossing"),
];

async fn registered_for(
    root: &Root,
    country: Country,
) -> Result<(Query, HashSet<&'static str>), CrossingError> {
    let query = Query::new(root.clone());
    let mut held = HashSet::new();
    for (dataset, table) in READ {
        if query.register_of_country(dataset, table, country).await? {
            held.insert(dataset.name);
        }
    }
    Ok((query, held))
}

async fn sessions_in(query: &Query) -> Result<Vec<Session>, CrossingError> {
    let stored: Vec<StoredSession> = query
        .rows("SELECT session_id, device_id, bbox FROM session")
        .await?;
    let samples: Vec<StoredSample> = query
        .rows(
            "SELECT session_id, t,
                    ST_X(geometry_projected) AS x, ST_Y(geometry_projected) AS y
             FROM session_sample
             ORDER BY t",
        )
        .await?;

    let mut by_session: HashMap<String, Vec<Sample>> = HashMap::new();
    for sample in samples {
        by_session
            .entry(sample.session_id.to_string())
            .or_default()
            .push(Sample {
                t: sample.t,
                projected: Point::new(sample.x, sample.y),
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
                bbox: session.bbox.rect(),
                samples,
            }
        })
        .collect())
}

async fn crossings_in(query: &Query) -> Result<Vec<Crossing>, CrossingError> {
    let stored: Vec<PlacedCrossing> = query
        .rows(
            "SELECT *,
                    ST_X(geometry_projected) AS x, ST_Y(geometry_projected) AS y,
                    ST_X(geometry) AS lon, ST_Y(geometry) AS lat
             FROM water_crossing",
        )
        .await?;

    stored
        .into_iter()
        .map(|crossing| {
            Ok(Crossing {
                crossing: domain::Crossing::at(
                    crossing.row.crossing_id,
                    crossing.lat,
                    crossing.lon,
                )?,
                projected: Point::new(crossing.x, crossing.y),
            })
        })
        .collect()
}

fn row(pass: &Pass, device: &DeviceId, radius: Radius) -> SessionCrossingRow {
    SessionCrossingRow {
        session_id: pass.session_id.clone(),
        crossing_id: pass.crossing_id.clone(),
        device_id: device.clone(),
        crossed_at: pass.crossed_at,
        distance_m: pass.distance_metres,
        samples_within: pass.samples_within,
        match_radius_m: radius.as_metres(),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use domain::CrossingId;

    use super::*;

    fn pass() -> Pass {
        Pass {
            session_id: SessionId::new("session-a").expect("an id"),
            crossing_id: CrossingId::new("water:track:rail@0.5").expect("an id"),
            crossed_at: DateTime::UNIX_EPOCH + TimeDelta::seconds(1),
            distance_metres: 20.0,
            samples_within: 3,
        }
    }

    #[test]
    fn a_row_records_the_radius_it_was_matched_under() {
        let device = DeviceId::new("device-a").expect("an id");

        let row = row(&pass(), &device, Radius::new(150.0));

        assert_eq!(row.match_radius_m, 150.0);
        assert_eq!(row.distance_m, 20.0);
    }

    #[test]
    fn a_row_names_the_device_the_session_ran_on() {
        let device = DeviceId::new("device-a").expect("an id");

        let row = row(&pass(), &device, Radius::new(150.0));

        assert_eq!(row.device_id, device);
        assert_eq!(row.session_id, pass().session_id);
    }
}
