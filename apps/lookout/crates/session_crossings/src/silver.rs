use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use domain::Bbox;
use domain::{DeviceId, Pass, SessionId};
use geo_types::Point;
use medallion::{COUNTRY, Country, DatasetSpec, Query, Replaced, Root, layers};
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
    #[error("partitioning by country: {0}")]
    Path(#[from] medallion::PathError),
}

#[derive(Debug, Deserialize)]
struct StoredSession {
    session_id: SessionId,
    device_id: DeviceId,
    bbox: Bbox,
    samples: Option<Vec<StoredSample>>,
}

#[derive(Debug, Deserialize)]
struct StoredSample {
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

    for country in countries_to_match(root)? {
        let (matched, rows) = matched_in(root, country, radius).await?;
        outcome.sessions += matched.sessions;
        outcome.crossings += matched.crossings;
        outcome.sessions_matched += matched.sessions_matched;
        passed.extend(rows);
    }

    passed.sort_by(|a, b| (a.crossed_at, &a.crossing_id).cmp(&(b.crossed_at, &b.crossing_id)));
    outcome.passes = passed.len();
    outcome.partitions = medallion::write_rows(root, &passed).await?.partitions;
    Ok(outcome)
}

fn datasets_held_in(root: &Root, country: Country) -> Result<Vec<&'static str>, CrossingError> {
    let mut held = Vec::new();
    for (dataset, _) in READ {
        if root
            .dataset(dataset)
            .partition(COUNTRY, country)?
            .is_filled()
        {
            held.push(dataset.name);
        }
    }
    Ok(held)
}

fn countries_to_match(root: &Root) -> Result<Vec<Country>, CrossingError> {
    let mut to_match = Vec::new();
    let mut held_by_some_country: HashSet<&'static str> = HashSet::new();

    for country in Country::ALL {
        let held = datasets_held_in(root, country)?;
        held_by_some_country.extend(&held);

        if held.len() == READ.len() {
            to_match.push(country);
        } else if !held.is_empty() {
            let missing = READ
                .iter()
                .map(|(dataset, _)| dataset.name)
                .filter(|name| !held.contains(name))
                .collect::<Vec<_>>()
                .join(", ");
            tracing::warn!(
                country = %country.code(),
                %missing,
                "a country holds only some of the datasets to match, so no passes are derived \
                 for it"
            );
        }
    }

    if let Some((dataset, _)) = READ
        .iter()
        .find(|(dataset, _)| !held_by_some_country.contains(dataset.name))
    {
        return Err(CrossingError::Missing {
            dataset: dataset.name,
        });
    }

    Ok(to_match)
}

async fn matched_in(
    root: &Root,
    country: Country,
    radius: Radius,
) -> Result<(MatchOutcome, Vec<SessionCrossingRow>), CrossingError> {
    let query = Query::new(root.clone());
    for (dataset, table) in READ {
        query.register_of_country(dataset, table, country).await?;
    }

    let sessions = sessions_in(&query).await?;
    let crossings = crossings_in(&query).await?;
    let matched = passes(&sessions, &crossings, radius);

    let devices: HashMap<&SessionId, &DeviceId> = sessions
        .iter()
        .map(|session| (&session.session_id, &session.device_id))
        .collect();
    let rows = matched
        .iter()
        .map(|pass| row(pass, devices[&pass.session_id], radius))
        .collect();

    Ok((
        MatchOutcome {
            sessions: sessions.len(),
            crossings: crossings.len(),
            sessions_matched: matched
                .iter()
                .map(|pass| &pass.session_id)
                .collect::<HashSet<_>>()
                .len(),
            ..MatchOutcome::default()
        },
        rows,
    ))
}

const READ: [(DatasetSpec<layers::Silver>, &str); 3] = [
    (medallion_model::SESSION, "session"),
    (medallion_model::SESSION_SAMPLE, "session_sample"),
    (medallion_model::WATER_CROSSING, "water_crossing"),
];

async fn sessions_in(query: &Query) -> Result<Vec<Session>, CrossingError> {
    let stored: Vec<StoredSession> = query
        .rows(
            "SELECT s.session_id, s.device_id, s.bbox, p.samples
             FROM session s
             LEFT JOIN (
                 SELECT session_id,
                        array_agg(
                            named_struct(
                                't', t,
                                'x', ST_X(geometry_projected),
                                'y', ST_Y(geometry_projected)
                            )
                            ORDER BY t
                        ) AS samples
                 FROM session_sample
                 GROUP BY session_id
             ) p ON s.session_id = p.session_id",
        )
        .await?;

    Ok(stored
        .into_iter()
        .map(|session| Session {
            session_id: session.session_id,
            device_id: session.device_id,
            bbox: session.bbox.rect(),
            samples: session
                .samples
                .unwrap_or_default()
                .into_iter()
                .map(|sample| Sample {
                    t: sample.t,
                    projected: Point::new(sample.x, sample.y),
                })
                .collect(),
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
