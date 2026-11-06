use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::time::Instant;

use domain::{CrossingCompactId, CrossingId, Percentage, SessionId};
use evaluation::{OfflineDetector, Pick, Seed, pick};
use medallion::{Country, Query, Root};
use medallion_model::{
    TRAIN_SESSION_CROSSING_V1, TrainSessionCrossingRowV1, TrainSessionRowV3, WaterCrossingRowV2,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Root::new(Root::default_path()?);
    let country = Country::Germany;

    if std::env::args().nth(1).as_deref() == Some("pick-only") {
        return pick_only(root, country).await;
    }

    let started = Instant::now();
    let query = Query::new(root.clone());
    query
        .register(TRAIN_SESSION_CROSSING_V1, "train_session_crossing_v1")
        .await?;
    let passes: Vec<TrainSessionCrossingRowV1> = query
        .rows(
            "SELECT session_id, crossing_id, crossed_at, distance_m, samples_within, match_radius_m \
             FROM train_session_crossing_v1",
        )
        .await?;
    println!("read {} passes in {:.2?}", passes.len(), started.elapsed());

    let started = Instant::now();
    let sessions: Vec<SessionId> = Query::new(root.clone())
        .read_of_country::<TrainSessionRowV3>(country)
        .await?
        .rows()
        .await?
        .into_iter()
        .map(|session| session.session_id)
        .collect();
    println!(
        "read {} sessions in {:.2?}",
        sessions.len(),
        started.elapsed()
    );

    let started = Instant::now();
    let compact_ids: BTreeMap<CrossingId, CrossingCompactId> = Query::new(root.clone())
        .read_of_country::<WaterCrossingRowV2>(country)
        .await?
        .rows()
        .await?
        .into_iter()
        .map(|crossing| (crossing.crossing_id, crossing.crossing_compact_id))
        .collect();
    println!(
        "read {} crossings' compact ids in {:.2?}",
        compact_ids.len(),
        started.elapsed()
    );

    let started = Instant::now();
    let every = OfflineDetector::per_session(sessions.clone(), passes.clone(), &compact_ids)?;
    println!(
        "built {} detectors, one per session, in {:.2?}",
        every.len(),
        started.elapsed()
    );
    drop(every);

    let started = Instant::now();
    let picked: BTreeSet<SessionId> = pick(
        &root,
        &Pick {
            country,
            realtime_at_least: Percentage::new(90.0)?,
            sessions: NonZeroUsize::new(1000).ok_or("1000 is not zero")?,
            seed: Seed(1),
        },
    )
    .await?
    .into_iter()
    .map(|session| session.session_id)
    .collect();
    let theirs: Vec<TrainSessionCrossingRowV1> = passes
        .into_iter()
        .filter(|pass| picked.contains(&pass.session_id))
        .collect();
    println!(
        "picked {} sessions holding {} passes in {:.2?}",
        picked.len(),
        theirs.len(),
        started.elapsed()
    );

    let started = Instant::now();
    let detectors = OfflineDetector::per_session(picked, theirs, &compact_ids)?;
    println!(
        "built {} detectors for the pick in {:.2?}",
        detectors.len(),
        started.elapsed()
    );
    Ok(())
}

async fn pick_only(root: Root, country: Country) -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    let picked: BTreeSet<SessionId> = pick(
        &root,
        &Pick {
            country,
            realtime_at_least: Percentage::new(90.0)?,
            sessions: NonZeroUsize::new(1000).ok_or("1000 is not zero")?,
            seed: Seed(1),
        },
    )
    .await?
    .into_iter()
    .map(|session| session.session_id)
    .collect();
    println!(
        "picked {} sessions in {:.2?}",
        picked.len(),
        started.elapsed()
    );

    let started = Instant::now();
    let query = Query::new(root.clone());
    query
        .register(TRAIN_SESSION_CROSSING_V1, "train_session_crossing_v1")
        .await?;
    let named: Vec<String> = picked
        .iter()
        .map(|session_id| format!("'{session_id}'"))
        .collect();
    let theirs: Vec<TrainSessionCrossingRowV1> = query
        .rows(&format!(
            "SELECT session_id, crossing_id, crossed_at, distance_m, samples_within, match_radius_m \
             FROM train_session_crossing_v1 WHERE session_id IN ({})",
            named.join(", ")
        ))
        .await?;
    println!(
        "read the pick's {} passes in the engine in {:.2?}",
        theirs.len(),
        started.elapsed()
    );

    let started = Instant::now();
    let compact_ids: BTreeMap<CrossingId, CrossingCompactId> = Query::new(root.clone())
        .read_of_country::<WaterCrossingRowV2>(country)
        .await?
        .rows()
        .await?
        .into_iter()
        .map(|crossing| (crossing.crossing_id, crossing.crossing_compact_id))
        .collect();
    println!(
        "read {} crossings' compact ids in {:.2?}",
        compact_ids.len(),
        started.elapsed()
    );

    let started = Instant::now();
    let detectors = OfflineDetector::per_session(picked, theirs, &compact_ids)?;
    println!(
        "built {} detectors for the pick in {:.2?}",
        detectors.len(),
        started.elapsed()
    );
    Ok(())
}
