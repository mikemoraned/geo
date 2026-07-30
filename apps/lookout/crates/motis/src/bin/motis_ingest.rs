//! `motis_ingest`: derive the silver `train_segment` dataset from the bronze motis
//! capture log. A thin wrapper around [`motis::ingest::ingest`]; run via
//! `just silver-motis-ingest`.

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
