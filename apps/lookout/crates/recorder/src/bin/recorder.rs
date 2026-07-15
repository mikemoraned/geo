//! Reads the upstash telemetry queue into a rerun `.rrd`: connects via
//! `LOOKOUT_REDIS_URL` and logs each sample to a rerun recording (accel x/y/z scalar
//! series, gps as a geo point plus lat/lon scalars, namespaced per device, `t` as the
//! timeline). Run via `just record`.
//!
//! Two modes (default `view-latest`, to avoid accidental data loss while iterating):
//!   - `view-latest`: non-destructively read the latest N samples and save.
//!   - `drain`: `BRPOP` every sample off the queue (destructive) until empty or Ctrl-C.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};
use rerun::archetypes::{GeoPoints, Scalars};
use rerun::{RecordingStream, RecordingStreamError};
use shared::Sample;

/// How long to block on `BRPOP` before treating the queue as drained.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5);

/// The timeline samples are logged against (device capture time).
const TIMELINE: &str = "time";

/// Default number of most-recent samples read in `view-latest`.
const DEFAULT_LIMIT: usize = 1000;

/// Default rerun recording path.
const DEFAULT_OUTPUT: &str = "lookout.rrd";

#[derive(Parser)]
#[command(about = "Read the lookout telemetry queue into a rerun .rrd")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Non-destructively read the latest samples and save (default; leaves the queue intact).
    ViewLatest {
        /// How many of the most recent samples to read.
        #[arg(long, default_value_t = DEFAULT_LIMIT)]
        limit: usize,
        /// Path to write the rerun recording to.
        #[arg(long, short, default_value = DEFAULT_OUTPUT)]
        output: PathBuf,
    },
    /// Remove every sample from the queue (destructive) and save.
    Drain {
        /// Path to write the rerun recording to.
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

    let rec = rerun::RecordingStreamBuilder::new("lookout")
        .save(command.output())
        .expect("create rerun recording");

    let url = std::env::var("LOOKOUT_REDIS_URL")
        .expect("LOOKOUT_REDIS_URL must be set — run via `just record`");
    let mut conn = telemetry::connect(&url)
        .await
        .expect("connect to telemetry redis");

    let count = match &command {
        Command::ViewLatest { limit, .. } => view_latest(&rec, &mut conn, *limit).await,
        Command::Drain { .. } => drain(&rec, &mut conn).await,
    };

    if let Err(err) = rec.flush_blocking() {
        tracing::error!(%err, "failed to flush rerun recording");
    }
    tracing::info!(count, output = %command.output().display(), "wrote recording");
}

/// Non-destructively log the latest `limit` samples, then return.
async fn view_latest(rec: &RecordingStream, conn: &mut redis::aio::MultiplexedConnection, limit: usize) -> u64 {
    tracing::info!(limit, "reading latest samples (non-destructive)");
    let samples = telemetry::latest_samples(conn, limit)
        .await
        .expect("read latest samples");
    for sample in &samples {
        if let Err(err) = log_sample(rec, sample) {
            tracing::error!(%err, "failed to log sample to rerun");
        }
    }
    samples.len() as u64
}

/// Destructively `BRPOP` samples until the queue drains empty or Ctrl-C.
async fn drain(rec: &RecordingStream, conn: &mut redis::aio::MultiplexedConnection) -> u64 {
    tracing::info!("draining telemetry queue (destructive; Ctrl-C to stop)");
    let mut count: u64 = 0;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!(count, "interrupted; stopping");
                break;
            }
            result = telemetry::brpop_sample(conn, IDLE_TIMEOUT) => match result {
                Ok(Some(sample)) => {
                    if let Err(err) = log_sample(rec, &sample) {
                        tracing::error!(%err, "failed to log sample to rerun");
                    }
                    count += 1;
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

/// Log one sample to the rerun recording at its capture time, under a per-device
/// entity path. Accel axes and gps components that are absent are simply skipped.
fn log_sample(rec: &RecordingStream, sample: &Sample) -> Result<(), RecordingStreamError> {
    rec.set_timestamp_secs_since_epoch(TIMELINE, sample.t as f64 / 1000.0);
    let id = sample.id;

    if let Some(accel) = &sample.accel {
        if let Some(x) = accel.x {
            rec.log(format!("device/{id}/accel/x"), &Scalars::single(x))?;
        }
        if let Some(y) = accel.y {
            rec.log(format!("device/{id}/accel/y"), &Scalars::single(y))?;
        }
        if let Some(z) = accel.z {
            rec.log(format!("device/{id}/accel/z"), &Scalars::single(z))?;
        }
    }

    if let Some(gps) = &sample.gps {
        rec.log(
            format!("device/{id}/gps"),
            &GeoPoints::from_lat_lon([(gps.lat, gps.lon)]),
        )?;
        rec.log(format!("device/{id}/gps/lat"), &Scalars::single(gps.lat))?;
        rec.log(format!("device/{id}/gps/lon"), &Scalars::single(gps.lon))?;
    }

    Ok(())
}
