use std::time::Duration;

use chrono::Utc;
use clap::Parser;
use tracing_subscriber::EnvFilter;
use url::Url;

use medallion::MedallionArgs;
use medallion_model::{Feed, MotisSource};
use motis::bronze::SegmentLog;
use motis::client::{DEFAULT_BASE_URL, MotisClient};
use motis::details::TripDetailsCache;
use motis::poll::{PollConfig, PollOutcome, Unresolved, poll_once};
use motis::source::register;
use motis::window::PositionWindow;

const DEFAULT_POLL_INTERVAL_SECS: u64 = 30;
const DEFAULT_WINDOW_AGE_MINS: u64 = 30;
const DEFAULT_RECENT_LOOKBACK_MINS: u64 = 5;
const DEFAULT_ZOOM: f64 = 8.0;

const SAMPLE_LIMIT: usize = 1000;

const QUERY_WINDOW_HALF_MINS: u64 = 5;

#[derive(Parser)]
#[command(about = "Poll Motis for train trips near recently logged GPS and log them")]
struct Args {
    /// Seconds between polls.
    #[arg(long, default_value_t = DEFAULT_POLL_INTERVAL_SECS)]
    poll_interval_secs: u64,
    /// Minutes a GPS position stays in the rolling window before it is pruned.
    #[arg(long, default_value_t = DEFAULT_WINDOW_AGE_MINS)]
    window_age_mins: u64,
    /// Only ingest GPS samples captured within the past this many minutes.
    #[arg(long, default_value_t = DEFAULT_RECENT_LOOKBACK_MINS)]
    recent_lookback_mins: u64,
    /// Motis zoom level (higher adds subway/tram/bus on top of long-distance rail).
    #[arg(long, default_value_t = DEFAULT_ZOOM)]
    zoom: f64,
    /// Base URL of the Motis server.
    #[arg(long, default_value = DEFAULT_BASE_URL)]
    motis_url: Url,
    /// Which feed the Motis server answers from.
    #[arg(long)]
    feed: Feed,
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "motis_poll=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args.medallion.root().expect("locate the medallion store");

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let client = MotisClient::new(args.motis_url.as_str().trim_end_matches('/'));
    let motis_version = client
        .motis_version()
        .await
        .expect("ask the Motis server for its version");
    let source = MotisSource {
        base_url: args.motis_url.clone(),
        motis_version,
        feed: args.feed,
        area: None,
    };
    let registration = register(&root, &source, Utc::now())
        .await
        .expect("record the Motis source");
    tracing::info!(source_id = %source.id(), ?registration, "recorded the Motis source");
    let log = SegmentLog::new(root.clone(), source.id());
    let mut details = TripDetailsCache::default();
    let mut window = PositionWindow::new(Duration::from_secs(args.window_age_mins * 60));
    let config = PollConfig {
        recent_lookback: Duration::from_secs(args.recent_lookback_mins * 60),
        query_window_half: Duration::from_secs(QUERY_WINDOW_HALF_MINS * 60),
        zoom: args.zoom,
        sample_limit: SAMPLE_LIMIT,
    };

    let url = std::env::var("LOOKOUT_REDIS_URL")
        .expect("LOOKOUT_REDIS_URL must be set — run via `just bronze-poll-motis`");
    let mut conn = telemetry::connect(&url)
        .await
        .expect("connect to telemetry redis");

    tracing::info!(
        motis_url = %args.motis_url,
        medallion_root = %root.path().display(),
        poll_interval_secs = args.poll_interval_secs,
        window_age_mins = args.window_age_mins,
        recent_lookback_mins = args.recent_lookback_mins,
        zoom = args.zoom,
        "starting motis poll loop (Ctrl-C to stop)"
    );

    let mut ticker = tokio::time::interval(Duration::from_secs(args.poll_interval_secs));
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("interrupted; stopping");
                break;
            }
            _ = ticker.tick() => {
                match poll_once(Utc::now(), &mut conn, &client, &log, &mut window, &mut details, &config).await {
                    Ok(PollOutcome::NoRecentGps { ingested }) => {
                        tracing::info!(ingested, "no recent gps positions; skipping motis query");
                    }
                    Ok(PollOutcome::Queried { ingested, positions, segments, unresolved }) => {
                        tracing::info!(ingested, positions, segments, unresolved = unresolved.len(), "polled motis");
                        for failure in &unresolved {
                            match failure {
                                Unresolved::Segment { reason } => {
                                    tracing::error!(%reason, "a segment was not captured");
                                }
                                Unresolved::Details { trip_id, reason } => {
                                    tracing::error!(%trip_id, %reason, "a trip was captured without its details");
                                }
                            }
                        }
                    }
                    Err(err) => tracing::error!(%err, "poll tick failed"),
                }
            }
        }
    }
}
