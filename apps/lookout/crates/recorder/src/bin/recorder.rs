use std::time::Duration;

use chrono::Utc;
use clap::{Parser, Subcommand};
use medallion::MedallionArgs;
use recorder::bronze::{Archive, Payload, Written};
use telemetry::RawSample;

const IDLE_TIMEOUT: Duration = Duration::from_secs(5);

const DEFAULT_LIMIT: usize = 1000;

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
    /// Read the latest samples and archive them without removing them from the queue (the
    /// default).
    PeekLatest {
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
    let command = args.command.unwrap_or(Command::PeekLatest {
        limit: DEFAULT_LIMIT,
    });

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let archive = Archive::new(root.clone());

    let url = std::env::var("LOOKOUT_REDIS_URL")
        .expect("LOOKOUT_REDIS_URL must be set — run via `just bronze-record`");
    let mut conn = telemetry::connect(&url)
        .await
        .expect("connect to telemetry redis");

    let written = match &command {
        Command::PeekLatest { limit } => peek_latest(&archive, &mut conn, *limit).await,
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

async fn peek_latest(
    archive: &Archive,
    conn: &mut redis::aio::MultiplexedConnection,
    limit: usize,
) -> Written {
    tracing::info!(limit, "peeking at the latest samples; nothing is removed");
    let samples = telemetry::peek_newest_samples(conn, limit)
        .await
        .expect("read latest samples");

    let mut total = Written::default();
    for batch in samples.chunks(BATCH_SIZE) {
        match write(archive, batch).await {
            Some(written) => total = total + written,
            None => break,
        }
    }
    total
}

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
            result = telemetry::take_oldest_sample(conn, IDLE_TIMEOUT) => match result {
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

async fn requeue(conn: &mut redis::aio::MultiplexedConnection, samples: &[RawSample]) {
    let mut requeued = 0;
    for sample in samples {
        match telemetry::requeue_as_oldest(conn, sample).await {
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
