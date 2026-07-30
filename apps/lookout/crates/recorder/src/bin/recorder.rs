//! Reads the upstash telemetry queue into the bronze telemetry datasets: connects via
//! `LOOKOUT_REDIS_URL` and writes the samples it reads — the verbatim payloads plus the
//! readings interpreted from them. Run via `just record`.
//!
//! Two modes (default `view-latest`, to avoid accidental data loss while iterating):
//!   - `view-latest`: non-destructively read the latest N samples and archive them.
//!   - `drain`: `BRPOP` every sample off the queue (destructive) until empty or Ctrl-C.
//!
//! A drain writes in batches of at most [`BATCH_SIZE`] rather than once at the end. `BRPOP`
//! has already removed what it holds, so a batch that fails to write is put back on the
//! queue — and if that requeue also fails the samples are gone. Writing in bounded batches
//! caps how many samples a single failure can put at risk, however long the drain runs.

use std::time::Duration;

use chrono::Utc;
use clap::{Parser, Subcommand};
use medallion::MedallionArgs;
use recorder::bronze::{Archive, Payload, Written};
use telemetry::RawSample;

/// How long to block on `BRPOP` before treating the queue as drained.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5);

/// Default number of most-recent samples read in `view-latest`.
const DEFAULT_LIMIT: usize = 1000;

/// Most samples held before being written. This bounds what a failed write followed by a
/// failed requeue can lose.
const BATCH_SIZE: usize = 100;

#[derive(Parser)]
#[command(about = "Read the lookout telemetry queue into the bronze telemetry datasets")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[derive(Subcommand)]
enum Command {
    /// Non-destructively read the latest samples and archive them (default; leaves the queue intact).
    ViewLatest {
        /// How many of the most recent samples to read.
        #[arg(long, default_value_t = DEFAULT_LIMIT)]
        limit: usize,
    },
    /// Remove every sample from the queue (destructive) and archive them.
    Drain,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "recorder=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args.medallion.root().expect("locate the medallion store");
    // Default to the non-destructive view-latest when no subcommand is given.
    let command = args.command.unwrap_or(Command::ViewLatest {
        limit: DEFAULT_LIMIT,
    });

    // rustls needs a process-global crypto provider before any `rediss://` connection.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let archive = Archive::new(root.clone());

    let url = std::env::var("LOOKOUT_REDIS_URL")
        .expect("LOOKOUT_REDIS_URL must be set — run via `just record`");
    let mut conn = telemetry::connect(&url)
        .await
        .expect("connect to telemetry redis");

    let written = match &command {
        Command::ViewLatest { limit } => view_latest(&archive, &mut conn, *limit).await,
        Command::Drain => drain(&archive, &mut conn).await,
    };

    tracing::info!(
        raw = written.raw,
        gps = written.gps,
        accel = written.accel,
        devices = written.devices,
        unparseable = written.unparseable,
        medallion_root = %root.path().display(),
        "wrote telemetry"
    );
}

/// Non-destructively archive the latest `limit` samples, in batches.
async fn view_latest(
    archive: &Archive,
    conn: &mut redis::aio::MultiplexedConnection,
    limit: usize,
) -> Written {
    tracing::info!(limit, "reading latest samples (non-destructive)");
    let samples = telemetry::latest_samples(conn, limit)
        .await
        .expect("read latest samples");

    let mut total = Written::default();
    for batch in samples.chunks(BATCH_SIZE) {
        match write(archive, batch).await {
            // Nothing was removed from the queue, so a failure loses nothing: report it
            // and stop.
            Some(written) => total = total + written,
            None => break,
        }
    }
    total
}

/// Destructively `BRPOP` samples until the queue drains empty or Ctrl-C, writing each
/// batch as it fills. A batch that fails to write is put back on the queue and the drain
/// stops, so the failure can't repeat across the rest of the queue.
async fn drain(archive: &Archive, conn: &mut redis::aio::MultiplexedConnection) -> Written {
    tracing::info!("draining telemetry queue (destructive; Ctrl-C to stop)");
    let mut total = Written::default();
    let mut batch: Vec<RawSample> = Vec::with_capacity(BATCH_SIZE);

    loop {
        let stop = tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("interrupted; stopping");
                true
            }
            result = telemetry::brpop_sample(conn, IDLE_TIMEOUT) => match result {
                Ok(Some(raw)) => {
                    batch.push(raw);
                    false
                }
                Ok(None) => {
                    tracing::info!("queue empty; stopping");
                    true
                }
                Err(err) => {
                    tracing::error!(%err, "error draining queue; stopping");
                    true
                }
            },
        };

        if stop || batch.len() >= BATCH_SIZE {
            if !batch.is_empty() {
                match write(archive, &batch).await {
                    Some(written) => total = total + written,
                    None => {
                        requeue(conn, &batch).await;
                        break;
                    }
                }
                batch.clear();
            }
            if stop {
                break;
            }
        }
    }
    total
}

/// Write one batch, reporting what landed, or `None` if it could not be written.
async fn write(archive: &Archive, samples: &[RawSample]) -> Option<Written> {
    let payloads: Vec<Payload> = samples.iter().map(Payload::from).collect();
    match archive.write(Utc::now(), &payloads).await {
        Ok(written) => Some(written),
        Err(err) => {
            tracing::error!(%err, count = samples.len(), "failed to write batch");
            None
        }
    }
}

/// Put a batch back on the queue after a failed write, so a drain that could not be
/// archived loses nothing.
async fn requeue(conn: &mut redis::aio::MultiplexedConnection, samples: &[RawSample]) {
    let mut requeued = 0;
    for sample in samples {
        match telemetry::requeue_sample(conn, sample).await {
            Ok(()) => requeued += 1,
            Err(err) => tracing::error!(%err, "failed to requeue sample — sample lost"),
        }
    }
    tracing::info!(
        requeued,
        of = samples.len(),
        "requeued batch after failed write"
    );
}
