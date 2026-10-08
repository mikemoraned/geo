use std::backtrace::{Backtrace, BacktraceStatus};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;

use tokio::signal::unix::{SignalKind, signal};
use tracing::Level;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::layer::{Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

use clap::{Args, Parser, Subcommand};
use object_store::path::Path as ObjectPath;
use overture_mirror::location::{Location, OVERTURE};
use overture_mirror::mirror::{self, LocalStatus, Source};
use overture_mirror::progress::Display;
use overture_mirror::release::{Release, find_superseded};
use overture_mirror::sync::{Refused, check_syncable, check_verifiable};

/// Mirror Overture Maps releases from the public bucket, or from one mirror to another.
#[derive(Parser)]
#[command(about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    /// The file each download, retry, and copied file is logged to. Defaults to a new file named
    /// for the run's start time.
    #[arg(long, global = true)]
    log: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// List the releases a source holds, and mark each superseded one.
    Releases {
        #[command(flatten)]
        source: SourceArg,
    },
    /// Copy one release whole into the mirror, resuming from the files already there.
    Sync(Transfer),
    /// Check a mirrored release against its source, copying nothing. Fails if any file is off.
    Verify(Transfer),
}

#[derive(Args)]
struct SourceArg {
    /// The releases to read: `s3://<bucket>/<prefix>`, or a mirror's directory.
    #[arg(long, default_value = OVERTURE)]
    source: Location,
}

#[derive(Args)]
struct Transfer {
    release: Release,
    #[command(flatten)]
    source: SourceArg,
    /// The directory holding one directory per mirrored release.
    #[arg(long)]
    mirror: PathBuf,
}

type Check = fn(&Release, &[Release], &Location, &Path) -> Result<(), Refused>;

impl Transfer {
    async fn open(&self, check: Check) -> Result<Source, Box<dyn Error>> {
        let location = &self.source.source;
        let source = Source::open(location)?;
        check(
            &self.release,
            &source.releases().await?,
            location,
            &self.mirror,
        )?;
        Ok(source)
    }
}

fn files_with_status(found: &BTreeMap<ObjectPath, LocalStatus>, status: LocalStatus) -> usize {
    found.values().filter(|each| **each == status).count()
}

#[derive(Debug, thiserror::Error)]
#[error(
    "{release} in the mirror is incomplete: {missing} missing, {differing} differing, {local_only} present locally alone"
)]
struct Incomplete {
    release: Release,
    missing: usize,
    differing: usize,
    local_only: usize,
}

impl Incomplete {
    fn is_empty(&self) -> bool {
        self.missing + self.differing + self.local_only == 0
    }
}

fn log_named_for_now() -> PathBuf {
    let started = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
    PathBuf::from(format!("overture-mirror-{started}.log"))
}

const SCREEN: &str = "screen";

fn open_log(path: &Path) -> Result<File, String> {
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|err| format!("opening the log {}: {err}", path.display()))
}

fn start_logging(log: Option<File>) {
    let screen = tracing_subscriber::fmt::layer()
        .without_time()
        .with_level(false)
        .with_target(false)
        .with_ansi(false)
        .log_internal_errors(false)
        .with_writer(
            std::io::stderr
                .with_max_level(Level::WARN)
                .or_else(std::io::stdout),
        )
        .with_filter(Targets::new().with_target(SCREEN, Level::INFO));
    let file = log.map(|log| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .log_internal_errors(false)
            .with_writer(Mutex::new(log))
            .with_filter(
                Targets::new()
                    .with_target("object_store", Level::INFO)
                    .with_target("overture_mirror", Level::INFO)
                    .with_target(SCREEN, Level::INFO),
            )
    });
    tracing_subscriber::registry()
        .with(screen)
        .with(file)
        .init();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Signal {
    name: &'static str,
    number: u8,
}

#[derive(Debug)]
enum Stopped {
    Failed(Box<dyn Error>),
    Signalled(Signal),
}

async fn signal_received() -> Result<Signal, Box<dyn Error>> {
    let watched = [
        ("SIGHUP", SignalKind::hangup(), 1),
        ("SIGINT", SignalKind::interrupt(), 2),
        ("SIGQUIT", SignalKind::quit(), 3),
        ("SIGTERM", SignalKind::terminate(), 15),
    ];
    let mut waits = Vec::new();
    for (name, kind, number) in watched {
        let mut stream = signal(kind)?;
        waits.push(Box::pin(async move {
            stream.recv().await;
            Signal { name, number }
        }));
    }
    Ok(futures::future::select_all(waits).await.0)
}

fn log_panics() {
    std::panic::set_hook(Box::new(|panic| {
        let backtrace = Backtrace::capture();
        match backtrace.status() {
            BacktraceStatus::Captured => {
                tracing::error!(target: SCREEN, "panicked: {panic}\n{backtrace}")
            }
            _ => tracing::error!(target: SCREEN, "panicked: {panic}"),
        }
    }));
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let log = open_log(&cli.log.clone().unwrap_or_else(log_named_for_now));
    let unopened = log.as_ref().err().cloned();
    start_logging(log.ok());
    if let Some(err) = unopened {
        tracing::error!(target: SCREEN, "failed: {err}");
        return ExitCode::FAILURE;
    }
    log_panics();
    let arguments: Vec<String> = std::env::args().collect();
    tracing::info!(
        "started, as process {}: {}",
        std::process::id(),
        arguments.join(" ")
    );
    let stopped = tokio::select! {
        outcome = run(cli.command) => outcome.map_err(Stopped::Failed),
        signal = signal_received() => Err(match signal {
            Ok(signal) => Stopped::Signalled(signal),
            Err(err) => Stopped::Failed(err),
        }),
    };
    match stopped {
        Ok(summary) => {
            tracing::info!(target: SCREEN, "finished: {summary}");
            ExitCode::SUCCESS
        }
        Err(Stopped::Failed(err)) => {
            tracing::error!(target: SCREEN, "failed: {err}");
            ExitCode::FAILURE
        }
        Err(Stopped::Signalled(Signal { name, number })) => {
            tracing::error!(target: SCREEN, "stopped by {name}");
            ExitCode::from(128 + number)
        }
    }
}

async fn run(command: Commands) -> Result<String, Box<dyn Error>> {
    match command {
        Commands::Releases { source } => {
            let served = Source::open(&source.source)?.releases().await?;
            let superseded = find_superseded(&served);
            for release in &served {
                match superseded.get(release) {
                    Some(latest) => {
                        tracing::info!(target: SCREEN, "{release}  superseded by {latest}")
                    }
                    None => tracing::info!(target: SCREEN, "{release}"),
                }
            }
            Ok(format!("listed {} releases", served.len()))
        }
        Commands::Sync(transfer) => {
            let source = transfer.open(check_syncable).await?;
            let Transfer {
                release, mirror, ..
            } = transfer;
            let found = mirror::sync(&source, &release, &mirror, Display::Bars).await?;
            Ok(format!(
                "{release}: {} already complete, {} copied, {} recopied",
                files_with_status(&found, LocalStatus::Complete),
                files_with_status(&found, LocalStatus::Missing),
                files_with_status(&found, LocalStatus::Differs)
            ))
        }
        Commands::Verify(transfer) => {
            let source = transfer.open(check_verifiable).await?;
            let Transfer {
                release, mirror, ..
            } = transfer;
            let verification = mirror::verify(&source, &release, &mirror, Display::Bars).await?;
            for (location, state) in &verification.remote {
                if *state != LocalStatus::Complete {
                    tracing::info!(target: SCREEN, "{state:?}  {location}");
                }
            }
            for path in &verification.local_only {
                tracing::info!(target: SCREEN, "LocalOnly  {}", path.display());
            }
            let incomplete = Incomplete {
                release: release.clone(),
                missing: files_with_status(&verification.remote, LocalStatus::Missing),
                differing: files_with_status(&verification.remote, LocalStatus::Differs),
                local_only: verification.local_only.len(),
            };
            if !incomplete.is_empty() {
                return Err(incomplete.into());
            }
            Ok(format!(
                "{release}: all {} files complete",
                verification.remote.len()
            ))
        }
    }
}
