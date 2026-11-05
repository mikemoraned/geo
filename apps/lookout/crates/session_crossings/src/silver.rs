use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use domain::{DeviceId, Pass, Radius, SessionId};
use futures::StreamExt;
use geo_types::Point;
use medallion::{COUNTRY, Country, DatasetSpec, Query, Replaced, Root, layers};
use medallion_model::{SessionCrossingRowV3, TrainSessionCrossingRowV1, WaterCrossingRowV2};
use serde::Deserialize;

use crate::matching::{Crossing, CrossingIndex, Matcher, NoPass, Sample, Session};

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
    #[error("matching sessions to crossings: {0}")]
    Matching(#[from] NoPass),
    #[error("counting the sessions: {0}")]
    Count(#[from] std::num::TryFromIntError),
}

#[derive(Debug, Deserialize)]
struct RankedSample {
    session_id: SessionId,
    rank: u64,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    t: DateTime<Utc>,
    x: f64,
    y: f64,
}

impl RankedSample {
    fn sample(&self) -> Sample {
        Sample {
            t: self.t,
            projected: Point::new(self.x, self.y),
        }
    }
}

fn ranked_samples() -> String {
    format!(
        "SELECT session_id,
            ROW_NUMBER() {order} AS rank,
            t,
            ST_X(geometry_projected) AS x,
            ST_Y(geometry_projected) AS y
        FROM samples",
        order = Sample::SESSION_ORDER_SQL,
    )
}

#[derive(Debug, Deserialize)]
struct StoredDevice {
    session_id: SessionId,
    device_id: DeviceId,
}

#[derive(Debug, Deserialize)]
struct PlacedCrossing {
    #[serde(flatten)]
    row: WaterCrossingRowV2,
    x: f64,
    y: f64,
    lon: f64,
    lat: f64,
}

#[derive(Debug, Clone, Copy)]
struct Sessions {
    sessions: DatasetSpec<layers::Silver>,
    samples: DatasetSpec<layers::Silver>,
}

const DEVICES: Sessions = Sessions {
    sessions: medallion_model::SESSION_V3,
    samples: medallion_model::SESSION_SAMPLE_V3,
};

const TRAINS: Sessions = Sessions {
    sessions: medallion_model::TRAIN_SESSION_V3,
    samples: medallion_model::TRAIN_SESSION_SAMPLE_V3,
};

impl Sessions {
    fn read(self) -> [DatasetSpec<layers::Silver>; 3] {
        [
            self.sessions,
            self.samples,
            medallion_model::WATER_CROSSING_V2,
        ]
    }
}

pub async fn derive_device(root: &Root, radius: Radius) -> Result<MatchOutcome, CrossingError> {
    let (mut outcome, passed) = matched(root, DEVICES, radius).await?;
    let devices = devices(root).await?;
    let rows: Vec<SessionCrossingRowV3> = passed
        .iter()
        .map(|pass| device_row(pass, &devices[&pass.session_id], radius))
        .collect();
    outcome.partitions = medallion::write_rows(root, &rows).await?.partitions;
    Ok(outcome)
}

pub async fn derive_train(root: &Root, radius: Radius) -> Result<MatchOutcome, CrossingError> {
    let (mut outcome, passed) = matched(root, TRAINS, radius).await?;
    let rows: Vec<TrainSessionCrossingRowV1> =
        passed.iter().map(|pass| train_row(pass, radius)).collect();
    outcome.partitions = medallion::write_rows(root, &rows).await?.partitions;
    Ok(outcome)
}

async fn matched(
    root: &Root,
    kind: Sessions,
    radius: Radius,
) -> Result<(MatchOutcome, Vec<Pass>), CrossingError> {
    let mut outcome = MatchOutcome::default();
    let mut passed: Vec<Pass> = Vec::new();

    for country in countries_to_match(root, kind)? {
        let (matched, passes) = matched_in(root, kind, country, radius).await?;
        outcome.sessions += matched.sessions;
        outcome.crossings += matched.crossings;
        outcome.sessions_matched += matched.sessions_matched;
        passed.extend(passes);
    }

    passed.sort_by(|a, b| (a.crossed_at, &a.crossing_id).cmp(&(b.crossed_at, &b.crossing_id)));
    outcome.passes = passed.len();
    Ok((outcome, passed))
}

async fn devices(root: &Root) -> Result<HashMap<SessionId, DeviceId>, CrossingError> {
    let query = Query::new(root.clone());
    query
        .register_at_without_geometry(&root.dataset(DEVICES.sessions), "session_v3")
        .await?;
    let stored: Vec<StoredDevice> = query
        .rows("SELECT session_id, device_id FROM session_v3")
        .await?;
    Ok(stored
        .into_iter()
        .map(|stored| (stored.session_id, stored.device_id))
        .collect())
}

fn datasets_held_in(
    root: &Root,
    kind: Sessions,
    country: Country,
) -> Result<Vec<&'static str>, CrossingError> {
    let mut held = Vec::new();
    for dataset in kind.read() {
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

fn countries_to_match(root: &Root, kind: Sessions) -> Result<Vec<Country>, CrossingError> {
    let mut to_match = Vec::new();
    let mut held_by_some_country: HashSet<&'static str> = HashSet::new();

    for country in Country::ALL {
        let held = datasets_held_in(root, kind, country)?;
        held_by_some_country.extend(&held);

        if held.len() == kind.read().len() {
            to_match.push(country);
        } else if !held.is_empty() {
            let missing = kind
                .read()
                .iter()
                .map(|dataset| dataset.name)
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

    if let Some(dataset) = kind
        .read()
        .into_iter()
        .find(|dataset| !held_by_some_country.contains(dataset.name))
    {
        return Err(CrossingError::Missing {
            dataset: dataset.name,
        });
    }

    Ok(to_match)
}

async fn matched_in(
    root: &Root,
    kind: Sessions,
    country: Country,
    radius: Radius,
) -> Result<(MatchOutcome, Vec<Pass>), CrossingError> {
    let query = registered(root, kind, country).await?;
    let index = CrossingIndex::new(crossings_in(&query).await?);
    let sessions = usize::try_from(
        query
            .count("SELECT COUNT(*) AS count FROM sessions")
            .await?,
    )?;

    let mut matcher = Matcher::new(&index, radius);
    let mut batches = query.row_batches::<RankedSample>(&ranked_samples()).await?;
    while let Some(batch) = batches.next().await {
        for ranked in batch? {
            matcher.observe(&ranked.session_id, ranked.rank, &ranked.sample());
        }
    }
    let matched = matcher.passes()?;

    Ok((
        MatchOutcome {
            sessions,
            crossings: index.crossings().len(),
            sessions_matched: matched
                .iter()
                .map(|pass| &pass.session_id)
                .collect::<HashSet<_>>()
                .len(),
            ..MatchOutcome::default()
        },
        matched,
    ))
}

#[derive(Debug, Clone)]
pub struct Inputs {
    pub sessions: Vec<Session>,
    pub crossings: Vec<Crossing>,
}

pub async fn inputs(root: &Root, country: Country) -> Result<Inputs, CrossingError> {
    let query = registered(root, DEVICES, country).await?;
    let ranked: Vec<RankedSample> = query
        .rows(&format!("{} ORDER BY session_id, rank", ranked_samples()))
        .await?;
    Ok(Inputs {
        sessions: ranked
            .chunk_by(|a, b| a.session_id == b.session_id)
            .map(|samples| Session {
                session_id: samples[0].session_id.clone(),
                samples: samples.iter().map(RankedSample::sample).collect(),
            })
            .collect(),
        crossings: crossings_in(&query).await?,
    })
}

async fn registered(root: &Root, kind: Sessions, country: Country) -> Result<Query, CrossingError> {
    let query = Query::new(root.clone());
    query
        .register_of_country(kind.sessions, "sessions", country)
        .await?;
    query
        .register_of_country(kind.samples, "samples", country)
        .await?;
    query
        .register_of_country(
            medallion_model::WATER_CROSSING_V2,
            "water_crossing_v2",
            country,
        )
        .await?;
    Ok(query)
}

async fn crossings_in(query: &Query) -> Result<Vec<Crossing>, CrossingError> {
    let stored: Vec<PlacedCrossing> = query
        .rows(
            "SELECT *,
                    ST_X(geometry_projected) AS x, ST_Y(geometry_projected) AS y,
                    ST_X(geometry) AS lon, ST_Y(geometry) AS lat
             FROM water_crossing_v2",
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

fn device_row(pass: &Pass, device: &DeviceId, radius: Radius) -> SessionCrossingRowV3 {
    SessionCrossingRowV3 {
        session_id: pass.session_id.clone(),
        crossing_id: pass.crossing_id.clone(),
        device_id: device.clone(),
        crossed_at: pass.crossed_at,
        distance_m: pass.distance_metres,
        samples_within: pass.samples_within,
        match_radius_m: radius.as_metres(),
    }
}

fn train_row(pass: &Pass, radius: Radius) -> TrainSessionCrossingRowV1 {
    TrainSessionCrossingRowV1 {
        session_id: pass.session_id.clone(),
        crossing_id: pass.crossing_id.clone(),
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

        let row = device_row(&pass(), &device, Radius::new(150.0).expect("a radius"));

        assert_eq!(row.match_radius_m, 150.0);
        assert_eq!(row.distance_m, 20.0);
    }

    #[test]
    fn a_train_row_records_the_pass_and_the_radius_it_was_matched_under() {
        let row = train_row(&pass(), Radius::new(150.0).expect("a radius"));

        assert_eq!(row.session_id, pass().session_id);
        assert_eq!(row.crossing_id, pass().crossing_id);
        assert_eq!(row.crossed_at, pass().crossed_at);
        assert_eq!(row.samples_within, 3);
        assert_eq!(row.distance_m, 20.0);
        assert_eq!(row.match_radius_m, 150.0);
    }

    #[test]
    fn a_row_names_the_device_the_session_ran_on() {
        let device = DeviceId::new("device-a").expect("an id");

        let row = device_row(&pass(), &device, Radius::new(150.0).expect("a radius"));

        assert_eq!(row.device_id, device);
        assert_eq!(row.session_id, pass().session_id);
    }
}
