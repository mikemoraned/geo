//! Reads the upstash telemetry queue into a SQLite archive: connects via
//! `LOOKOUT_REDIS_URL` and writes each sample to the DB (a lossless `raw` table plus
//! derived per-sensor `accel`/`gps` tables). Run via `just record`.
//!
//! Two modes (default `view-latest`, to avoid accidental data loss while iterating):
//!   - `view-latest`: non-destructively read the latest N samples and archive them.
//!   - `drain`: `BRPOP` every sample off the queue (destructive) until empty or Ctrl-C.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};
use recorder::store::Store;
use telemetry::RawSample;

/// How long to block on `BRPOP` before treating the queue as drained.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5);

/// Default number of most-recent samples read in `view-latest`.
const DEFAULT_LIMIT: usize = 1000;

/// Default SQLite archive path.
const DEFAULT_OUTPUT: &str = "data/lookout.sqlite";

#[derive(Parser)]
#[command(about = "Read the lookout telemetry queue into a SQLite archive")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Non-destructively read the latest samples and archive them (default; leaves the queue intact).
    ViewLatest {
        /// How many of the most recent samples to read.
        #[arg(long, default_value_t = DEFAULT_LIMIT)]
        limit: usize,
        /// Path to the SQLite archive to write to.
        #[arg(long, short, default_value = DEFAULT_OUTPUT)]
        output: PathBuf,
    },
    /// Remove every sample from the queue (destructive) and archive them.
    Drain {
        /// Path to the SQLite archive to write to.
        #[arg(long, short, default_value = DEFAULT_OUTPUT)]
        output: PathBuf,
    },
}

impl Command {
    fn output(&self) -> &Path {
        match self {
            Self::ViewLatest { output, .. } | Self::Drain { output } => output,
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "recorder=info".into()),
        )
        .init();

    // Default to the non-destructive view-latest when no subcommand is given.
    let command = Args::parse().command.unwrap_or(Command::ViewLatest {
        limit: DEFAULT_LIMIT,
        output: PathBuf::from(DEFAULT_OUTPUT),
    });

    // rustls needs a process-global crypto provider before any `rediss://` connection.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let store = Store::open(command.output()).expect("open sqlite archive");

    let url = std::env::var("LOOKOUT_REDIS_URL")
        .expect("LOOKOUT_REDIS_URL must be set — run via `just record`");
    let mut conn = telemetry::connect(&url)
        .await
        .expect("connect to telemetry redis");

    let count = match &command {
        Command::ViewLatest { limit, .. } => view_latest(&store, &mut conn, *limit).await,
        Command::Drain { .. } => drain(&store, &mut conn).await,
    };

    tracing::info!(count, output = %command.output().display(), "wrote archive");
}

/// Non-destructively archive the latest `limit` samples, then return the count stored.
async fn view_latest(
    store: &Store,
    conn: &mut redis::aio::MultiplexedConnection,
    limit: usize,
) -> u64 {
    tracing::info!(limit, "reading latest samples (non-destructive)");
    let samples = telemetry::latest_samples(conn, limit)
        .await
        .expect("read latest samples");
    samples.iter().filter(|raw| archive(store, raw)).count() as u64
}

/// Destructively `BRPOP` samples until the queue drains empty or Ctrl-C.
async fn drain(store: &Store, conn: &mut redis::aio::MultiplexedConnection) -> u64 {
    tracing::info!("draining telemetry queue (destructive; Ctrl-C to stop)");
    let mut count: u64 = 0;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!(count, "interrupted; stopping");
                break;
            }
            result = telemetry::brpop_sample(conn, IDLE_TIMEOUT) => match result {
                Ok(Some(raw)) => {
                    if archive(store, &raw) {
                        count += 1;
                    }
                }
                Ok(None) => {
                    tracing::info!(count, "queue empty; stopping");
                    break;
                }
                Err(err) => {
                    tracing::error!(%err, "error draining queue; stopping");
                    break;
                }
            },
        }
    }
    count
}

/// Archive one payload, logging (rather than propagating) a write failure so a
/// single bad sample never aborts a drain. Returns whether it was stored.
fn archive(store: &Store, raw: &RawSample) -> bool {
    match store.insert(raw) {
        Ok(()) => true,
        Err(err) => {
            tracing::error!(%err, "failed to archive sample");
            false
        }
    }
}
