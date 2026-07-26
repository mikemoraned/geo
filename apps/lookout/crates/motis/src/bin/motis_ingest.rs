//! `motis_ingest`: derive the silver `train_segment` dataset from the bronze motis
//! capture log. A thin wrapper around [`motis::ingest::ingest`]; run via
//! `just ingest-motis`.

use clap::Parser;
use tracing_subscriber::EnvFilter;

use medallion::MedallionArgs;
use motis::ingest::ingest;

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

    let outcome = ingest(&args.medallion.root())
        .await
        .expect("derive train segments");

    tracing::info!(
        read = outcome.read,
        deduped = outcome.deduped,
        partitions = outcome.partitions,
        medallion_root = %args.medallion.medallion_root.display(),
        "derived train segments"
    );
}
