//! `motis_ingest`: dedup + decode the raw `motis` capture log into the derived
//! `train_segment` table of the `lookout` db. A thin wrapper around
//! [`motis::ingest::ingest`]; run via `just ingest-motis`.

use std::path::PathBuf;

use clap::Parser;
use rusqlite::Connection;
use tracing_subscriber::EnvFilter;

use motis::ingest::ingest;

const DEFAULT_MOTIS_DB: &str = "data/motis.sqlite";
const DEFAULT_LOOKOUT_DB: &str = "data/lookout.sqlite";

#[derive(Parser)]
#[command(about = "Dedup + decode the raw motis capture log into lookout's train_segment table")]
struct Args {
    /// Raw `motis` capture db to read.
    #[arg(long, default_value = DEFAULT_MOTIS_DB)]
    motis_db: PathBuf,
    /// `lookout` db to write the derived `train_segment` table into.
    #[arg(long, default_value = DEFAULT_LOOKOUT_DB)]
    lookout_db: PathBuf,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "motis_ingest=info".into()),
        )
        .init();

    let args = Args::parse();
    let source = Connection::open(&args.motis_db).expect("open motis db");
    let dest = Connection::open(&args.lookout_db).expect("open lookout db");

    let outcome = ingest(&source, &dest).expect("ingest capture log");

    tracing::info!(
        deduped = outcome.deduped,
        written = outcome.written,
        motis_db = %args.motis_db.display(),
        lookout_db = %args.lookout_db.display(),
        "ingested train segments"
    );
}
