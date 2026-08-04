//! `match_crossings`: derive the silver `session_crossing` dataset — the crossings each
//! recorded session passed, matched by how near the session's samples come to one.
//!
//! Reads the silver sessions and water crossings, so both have to have been derived. Every
//! session is matched again, so a rerun replaces what the last one wrote.

use clap::Parser;
use tracing_subscriber::EnvFilter;

use medallion::MedallionArgs;
use session_crossings::matching::Radius;
use session_crossings::silver;

#[derive(Parser)]
#[command(about = "Derive the crossings each session passed")]
struct Args {
    /// How near a sample has to come to a crossing, in metres, for it to count as passed.
    #[arg(long, default_value_t = Radius::default().as_metres())]
    match_radius_m: f64,
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "match_crossings=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args.medallion.root().expect("locate the medallion store");

    let outcome = silver::derive(&root, Radius::new(args.match_radius_m))
        .await
        .expect("derive the crossings each session passed");

    tracing::info!(
        sessions = outcome.sessions,
        crossings = outcome.crossings,
        sessions_matched = outcome.sessions_matched,
        passes = outcome.passes,
        partitions = outcome.partitions.written,
        partitions_removed = outcome.partitions.removed,
        match_radius_m = args.match_radius_m,
        medallion_root = %root.path().display(),
        "derived the crossings each session passed"
    );
}
