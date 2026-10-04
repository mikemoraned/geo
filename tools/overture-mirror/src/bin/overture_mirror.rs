use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;

use tracing::Level;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;
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
    /// The file each retry, resumed download, and copied file is logged to.
    #[arg(long, global = true, default_value = "overture-mirror.log")]
    log: PathBuf,
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

fn log_to(path: &Path) -> Result<(), Box<dyn Error>> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| format!("opening the log {}: {err}", path.display()))?;
    let targets = Targets::new()
        .with_target("object_store", Level::INFO)
        .with_target("overture_mirror", Level::INFO);
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(Mutex::new(file)),
        )
        .with(targets)
        .init();
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Err(err) = log_to(&cli.log) {
        eprintln!("{err}");
        return ExitCode::FAILURE;
    }
    match run(cli.command).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

async fn run(command: Commands) -> Result<(), Box<dyn Error>> {
    match command {
        Commands::Releases { source } => {
            let served = Source::open(&source.source)?.releases().await?;
            let superseded = find_superseded(&served);
            for release in &served {
                match superseded.get(release) {
                    Some(latest) => println!("{release}  superseded by {latest}"),
                    None => println!("{release}"),
                }
            }
        }
        Commands::Sync(transfer) => {
            let source = transfer.open(check_syncable).await?;
            let Transfer {
                release, mirror, ..
            } = transfer;
            let found = mirror::sync(&source, &release, &mirror, Display::Bars).await?;
            println!(
                "{release}: {} already complete, {} copied, {} recopied",
                files_with_status(&found, LocalStatus::Complete),
                files_with_status(&found, LocalStatus::Missing),
                files_with_status(&found, LocalStatus::Differs)
            );
        }
        Commands::Verify(transfer) => {
            let source = transfer.open(check_verifiable).await?;
            let Transfer {
                release, mirror, ..
            } = transfer;
            let verification = mirror::verify(&source, &release, &mirror, Display::Bars).await?;
            for (location, state) in &verification.remote {
                if *state != LocalStatus::Complete {
                    println!("{state:?}  {location}");
                }
            }
            for path in &verification.local_only {
                println!("LocalOnly  {}", path.display());
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
            println!(
                "{release}: all {} files complete",
                verification.remote.len()
            );
        }
    }
    Ok(())
}
