use std::num::NonZeroUsize;
use std::time::Instant;

use crossings::silver::read_of_country;
use domain::Percentage;
use evaluation::{IndexedCrossings, Pick, Seed, compact_ids, compacted, pick, replay};
use medallion::{Country, Root};
use predictor::{CrowFlies, DEFAULT_RADIUS};

#[tokio::main]
async fn main() {
    let root = Root::new("data/medallion");
    let country = Country::Germany;
    let sessions = pick(
        &root,
        &Pick {
            country,
            realtime_at_least: Percentage::new(90.0).expect("a percentage"),
            sessions: NonZeroUsize::new(1000).expect("non-zero"),
            seed: Seed(1),
        },
    )
    .await
    .expect("a pick");
    let crossings = read_of_country(&root, country).await.expect("crossings");
    let ids = compact_ids(&crossings);
    let compact = compacted::<f32>(&crossings).expect("compacted");

    let started = Instant::now();
    let scanned = replay(&root, country, &sessions, &ids, |_| {
        CrowFlies::new(compact.clone(), DEFAULT_RADIUS)
    })
    .await
    .expect("a scanned replay");
    println!("scanned: {:?}", started.elapsed());

    let indexed_crossings = IndexedCrossings::new(&compact);
    let started = Instant::now();
    let indexed = replay(&root, country, &sessions, &ids, |_| {
        CrowFlies::new(indexed_crossings.clone(), DEFAULT_RADIUS)
    })
    .await
    .expect("an indexed replay");
    println!("indexed: {:?}", started.elapsed());

    let transitions: usize = indexed.values().map(Vec::len).sum();
    println!(
        "{transitions} transitions, the same from both: {}",
        scanned == indexed
    );
}
