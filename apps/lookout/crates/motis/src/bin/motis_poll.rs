//! `motis_poll`: polls recent GPS positions off the redis telemetry queue, queries the
//! local Motis server for train trips within a buffered bounding box around them, and
//! appends the returned segments to a raw, duplication-allowed `motis` SQLite log.
//!
//! This is a thin loop around [`motis::poll::poll_once`] — argument parsing, redis/store
//! setup, the tick timer, and logging. Run via `just poll-motis`; Ctrl-C stops cleanly.

use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use motis::client::{MotisClient, DEFAULT_BASE_URL};
use motis::poll::{poll_once, PollConfig, PollOutcome};
use motis::store::Store;
use motis::window::PositionWindow;

const DEFAULT_POLL_INTERVAL_SECS: u64 = 30;
const DEFAULT_WINDOW_AGE_MINS: u64 = 30;
const DEFAULT_RECENT_LOOKBACK_MINS: u64 = 5;
const DEFAULT_ZOOM: f64 = 8.0;
const DEFAULT_DB: &str = "data/motis.sqlite";

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
    /// Path to the raw `motis` capture SQLite db.
    #[arg(long, default_value = DEFAULT_DB)]
    db: PathBuf,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "motis_poll=info".into()),
        )
        .init();

    let args = Args::parse();

    // rustls needs a process-global crypto provider before any `rediss://` connection.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let store = Store::open(&args.db).expect("open motis sqlite db");
    let client = MotisClient::new(&args.motis_url);
    let mut window = PositionWindow::new(Duration::from_secs(args.window_age_mins * 60));
    let config = PollConfig {
        recent_lookback: Duration::from_secs(args.recent_lookback_mins * 60),
        query_window_half: Duration::from_secs(QUERY_WINDOW_HALF_MINS * 60),
        zoom: args.zoom,
        sample_limit: SAMPLE_LIMIT,
    };

    let url = std::env::var("LOOKOUT_REDIS_URL")
        .expect("LOOKOUT_REDIS_URL must be set — run via `just poll-motis`");
    let mut conn = telemetry::connect(&url)
        .await
        .expect("connect to telemetry redis");

    tracing::info!(
        motis_url = %args.motis_url,
        db = %args.db.display(),
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
                match poll_once(Utc::now(), &mut conn, &client, &store, &mut window, &config).await {
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
