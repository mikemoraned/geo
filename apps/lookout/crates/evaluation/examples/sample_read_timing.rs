use std::num::NonZeroUsize;
use std::time::Instant;

use datafusion::prelude::col;
use domain::Percentage;
use evaluation::{Pick, Seed, pick};
use medallion::{Country, Query, Root};
use medallion_model::{TrainSessionSamplePositionV3, TrainSessionSampleRowV3};

#[tokio::main]
async fn main() {
    let root = Root::new("data/medallion");
    let sessions = pick(
        &root,
        &Pick {
            country: Country::Germany,
            realtime_at_least: Percentage::new(90.0).expect("a percentage"),
            sessions: NonZeroUsize::new(1000).expect("non-zero"),
            seed: Seed(1),
        },
    )
    .await
    .expect("a pick");

    let started = Instant::now();
    let picked = col("session_id").in_list(
        sessions
            .iter()
            .map(|session| datafusion::prelude::lit(session.session_id.to_string()))
            .collect(),
        false,
    );
    let rows = Query::new(root.clone())
        .read_of_country::<TrainSessionSampleRowV3>(Country::Germany)
        .await
        .expect("a read")
        .filter(picked)
        .expect("a filter")
        .sort(vec![
            col("session_id").sort(true, false),
            col("t").sort(true, false),
            col("seq").sort(true, false),
        ])
        .expect("a sort")
        .rows_as::<TrainSessionSamplePositionV3>()
        .await
        .expect("rows");
    println!("{} samples in {:?}", rows.len(), started.elapsed());
}
