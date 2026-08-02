//! `motis_ingest`: derive the silver `train_segment` dataset from the bronze motis capture
//! log — dedup legs seen by more than one poll, decode each polyline to WKB, and add the
//! projected geometry.
//!
//! A leg's country, and so the zone it is projected into, comes from where it starts,
//! resolved against the country areas of the newest Overture extract: a store without an
//! extract cannot be ingested.
//!
//! Only the partitions the capture log covers are rewritten, so a rerun over unchanged
//! bronze leaves the same dataset.

use clap::Parser;
use tracing_subscriber::EnvFilter;

use medallion::MedallionArgs;
use motis::ingest::ingest;
use transport::countries::CountryAreas;

#[derive(Parser)]
#[command(about = "Derive the silver train_segment dataset from the bronze motis capture log")]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "motis_ingest=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args.medallion.root().expect("locate the medallion store");

    let countries = CountryAreas::newest(&root)
        .await
        .expect("read the country areas of the newest extract");
    let outcome = ingest(&root, &countries)
        .await
        .expect("derive train segments");

    tracing::info!(
        read = outcome.read,
        deduped = outcome.deduped,
        partitions = outcome.partitions,
        unplaceable = outcome.unplaceable,
        medallion_root = %root.path().display(),
        "derived train segments"
    );
}
