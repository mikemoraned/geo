//! `backfill_segments`: read the pre-medallion sqlite capture log into the bronze capture
//! log, so the polls it holds aren't stranded behind the old format. A thin wrapper around
//! [`motis::backfill::backfill`]; run via `just backfill`.
//!
//! Adds only the polls bronze does not already hold, so running it again does nothing.

use std::path::PathBuf;

use clap::Parser;
use medallion::MedallionArgs;
use motis::backfill::backfill;
use tracing_subscriber::EnvFilter;

/// Where the old poller kept its capture log.
const DEFAULT_DB: &str = "data/motis.sqlite";

#[derive(Parser)]
#[command(about = "Read the pre-medallion sqlite capture log into the bronze capture log")]
struct Args {
    /// The sqlite capture log to read.
    #[arg(long, default_value = DEFAULT_DB)]
    db: PathBuf,
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "backfill_segments=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args.medallion.root().expect("locate the medallion store");

    let outcome = match backfill(&args.db, &root).await {
        Ok(outcome) => outcome,
        Err(err) => {
            tracing::error!(%err, "nothing was backfilled");
            std::process::exit(1);
        }
    };

    tracing::info!(
        read = outcome.read,
        polls = outcome.polls,
        written = outcome.written,
        skipped = outcome.skipped,
        db = %args.db.display(),
        medallion_root = %root.path().display(),
        "backfilled motis segments"
    );
}
