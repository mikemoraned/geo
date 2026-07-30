//! `backfill_telemetry`: read the pre-medallion sqlite archive into the bronze telemetry
//! datasets, so the history it holds isn't stranded behind the old format. A thin wrapper
//! around [`recorder::backfill::backfill`]; run via `just backfill`.
//!
//! Run once. Bronze is immutable, so a second run would leave every payload in it twice;
//! the backfill refuses that rather than relying on being run carefully.

use std::path::PathBuf;

use chrono::Utc;
use clap::Parser;
use medallion::MedallionArgs;
use recorder::backfill::backfill;
use tracing_subscriber::EnvFilter;

/// Where the old recorder kept its archive.
const DEFAULT_DB: &str = "data/lookout.sqlite";

#[derive(Parser)]
#[command(about = "Read the pre-medallion sqlite archive into the bronze telemetry datasets")]
struct Args {
    /// The sqlite archive to read.
    #[arg(long, default_value = DEFAULT_DB)]
    db: PathBuf,
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "backfill_telemetry=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args.medallion.root().expect("locate the medallion store");

    // Refusing to run is an expected outcome, not a crash: report why and stop.
    let outcome = match backfill(&args.db, &root, Utc::now()).await {
        Ok(outcome) => outcome,
        Err(err) => {
            tracing::error!(%err, "nothing was backfilled");
            std::process::exit(1);
        }
    };

    tracing::info!(
        read = outcome.read,
        raw = outcome.written.raw,
        gps = outcome.written.gps,
        accel = outcome.written.accel,
        devices = outcome.written.devices,
        unparseable = outcome.written.unparseable,
        undated = outcome.undated,
        db = %args.db.display(),
        medallion_root = %root.path().display(),
        "backfilled telemetry"
    );
}
