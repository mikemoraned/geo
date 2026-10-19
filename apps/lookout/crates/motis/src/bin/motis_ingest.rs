use clap::Parser;
use tracing_subscriber::EnvFilter;

use medallion::MedallionArgs;
use motis::ingest::ingest;
use motis::span::Span;
use motis::train_sessions::derive_train_sessions;
use transport::countries::CountryAreas;

#[derive(Parser)]
#[command(
    about = "Derive the silver train_segment_v2, train_session and train_session_sample datasets from the bronze motis capture logs"
)]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
    /// How often a train session samples where its train is, such as `10s`.
    #[arg(long, default_value = "10s")]
    sample_interval: Span,
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

    let countries = CountryAreas::newest_per_country(&root)
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
        shapeless = outcome.shapeless,
        shape_corrected = outcome.shape_corrected,
        medallion_root = %root.path().display(),
        "derived train segments"
    );

    let sessions = derive_train_sessions(&root, &countries, args.sample_interval)
        .await
        .expect("derive train sessions");

    tracing::info!(
        trips = sessions.trips,
        sessions = sessions.sessions,
        samples = sessions.samples,
        session_partitions = sessions.session_partitions.written,
        sample_partitions = sessions.sample_partitions.written,
        "derived train sessions"
    );
    tracing::info!(
        total = sessions.dropped.total(),
        shapeless = sessions.dropped.shapeless,
        arrives_before_departing = sessions.dropped.arrives_before_departing,
        overlapping = sessions.dropped.overlapping,
        unplaceable = sessions.dropped.unplaceable,
        "dropped trips"
    );
}
