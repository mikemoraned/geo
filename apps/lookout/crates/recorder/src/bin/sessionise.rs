//! `sessionise`: derive the silver `session` and `session_sample` datasets from the bronze
//! telemetry — dedup the GPS readings, split each device's into sessions, and write each
//! session's path and samples with their projected geometry.
//!
//! A session's country, and so the zone its geometry is projected into, comes from where it
//! started, resolved against the country areas of the newest Overture extract: a store
//! without an extract cannot be sessionised.
//!
//! Every session is re-derived from all of bronze, so a rerun replaces what the last one
//! wrote rather than adding to it.

use clap::Parser;
use tracing_subscriber::EnvFilter;

use medallion::MedallionArgs;
use recorder::sessions::{Gap, Lead, sessions};
use recorder::silver;
use transport::countries::CountryAreas;

#[derive(Parser)]
#[command(about = "Derive the silver session datasets from the bronze telemetry")]
struct Args {
    /// How long a device may go unheard before the silence separates two sessions.
    #[arg(long, default_value_t = Gap::default().as_seconds() / 60)]
    gap_mins: u32,
    /// How long before reporting a session a device may already have been fixing its
    /// position: samples this close ahead of a report open the session it reports.
    #[arg(long, default_value_t = Lead::default().as_seconds())]
    lead_secs: u32,
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "sessionise=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args.medallion.root().expect("locate the medallion store");
    let gap = Gap::new(chrono::Duration::minutes(i64::from(args.gap_mins)));
    let lead = Lead::new(chrono::Duration::seconds(i64::from(args.lead_secs)));

    let countries = CountryAreas::newest(&root)
        .await
        .expect("read the country areas of the newest extract");
    let derived = sessions(&root, gap, lead).await.expect("derive sessions");
    let outcome = silver::write(&root, &derived, &countries)
        .await
        .expect("write sessions");

    tracing::info!(
        sessions = outcome.sessions,
        samples = outcome.samples,
        session_partitions = outcome.session_partitions.written,
        session_partitions_removed = outcome.session_partitions.removed,
        sample_partitions = outcome.sample_partitions.written,
        sample_partitions_removed = outcome.sample_partitions.removed,
        unplaceable = outcome.unplaceable,
        gap_mins = args.gap_mins,
        lead_secs = args.lead_secs,
        medallion_root = %root.path().display(),
        "derived sessions"
    );
}
