//! `motis_ingest`: derive the silver `train_segment` dataset from the bronze motis
//! capture log. A thin wrapper around [`motis::ingest::ingest`]; run via
//! `just ingest-motis`.

use clap::Parser;
use tracing_subscriber::EnvFilter;

use medallion::{Country, MedallionArgs};
use motis::ingest::ingest;

#[derive(Parser)]
#[command(about = "Derive the silver train_segment dataset from the bronze motis capture log")]
struct Args {
    /// ISO 3166-1 alpha-2 code of the country the captured segments run in. It fixes the
    /// CRS of the derived projected geometry, so it is stated rather than assumed.
    #[arg(long)]
    country: Country,
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

    let outcome = ingest(&root, args.country)
        .await
        .expect("derive train segments");

    tracing::info!(
        read = outcome.read,
        deduped = outcome.deduped,
        partitions = outcome.partitions,
        country = %args.country,
        medallion_root = %root.path().display(),
        "derived train segments"
    );
}
