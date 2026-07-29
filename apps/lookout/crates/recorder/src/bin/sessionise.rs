//! `sessionise`: derive the silver `session` and `session_sample` datasets from the bronze
//! telemetry. A thin wrapper around [`recorder::sessions`] and [`recorder::silver`]; run via
//! `just sessionise`.

use clap::Parser;
use tracing_subscriber::EnvFilter;

use medallion::MedallionArgs;
use recorder::sessions::{sessions, Gap};
use recorder::silver;
use transport::countries::CountryAreas;

#[derive(Parser)]
#[command(about = "Derive the silver session datasets from the bronze telemetry")]
struct Args {
    /// How long a device may go unheard before the silence separates two sessions.
    #[arg(long, default_value_t = Gap::default().as_seconds() / 60)]
    gap_mins: u32,
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
    let root = args.medallion.root();
    let gap = Gap::new(chrono::Duration::minutes(i64::from(args.gap_mins)));

    let countries = CountryAreas::newest(&root)
        .await
        .expect("read the country areas of the newest extract");
    let derived = sessions(&root, gap).await.expect("derive sessions");
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
        medallion_root = %args.medallion.medallion_root.display(),
        "derived sessions"
    );
}
