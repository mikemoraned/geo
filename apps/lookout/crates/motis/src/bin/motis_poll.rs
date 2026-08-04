//! `motis_poll`: polls recent GPS positions off the redis telemetry queue, queries the
//! local Motis server for train trips within a buffered bounding box around them, and
//! writes the returned segments to the bronze capture log, one parquet file per poll.
//!
//! Runs a continuous loop until interrupted; Ctrl-C stops it cleanly, between polls.

use std::time::Duration;

use chrono::Utc;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use medallion::MedallionArgs;
use motis::bronze::SegmentLog;
use motis::client::{DEFAULT_BASE_URL, MotisClient};
use motis::poll::{PollConfig, PollOutcome, poll_once};
use motis::window::PositionWindow;

const DEFAULT_POLL_INTERVAL_SECS: u64 = 30;
const DEFAULT_WINDOW_AGE_MINS: u64 = 30;
const DEFAULT_RECENT_LOOKBACK_MINS: u64 = 5;
const DEFAULT_ZOOM: f64 = 8.0;

/// How many of the most-recent queued samples to scan for GPS each tick.
const SAMPLE_LIMIT: usize = 1000;

/// Half-width (minutes) of the `map/trips` time window queried around now.
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
    motis_url: String,
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

    // rustls needs a process-global crypto provider before any `rediss://` connection.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let log = SegmentLog::new(root.clone());
    let client = MotisClient::new(&args.motis_url);
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
                match poll_once(Utc::now(), &mut conn, &client, &log, &mut window, &config).await {
                    Ok(PollOutcome::NoRecentGps { ingested }) => {
                        tracing::info!(ingested, "no recent gps positions; skipping motis query");
                    }
                    Ok(PollOutcome::Queried { ingested, positions, segments }) => {
                        tracing::info!(ingested, positions, segments, "polled motis");
                    }
                    Err(err) => tracing::error!(%err, "poll tick failed"),
                }
            }
        }
    }
}
