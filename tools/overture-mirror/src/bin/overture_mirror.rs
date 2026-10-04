use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use overture_mirror::location::{Location, OVERTURE};
use overture_mirror::mirror::{self, Local, Source};
use overture_mirror::progress::Display;
use overture_mirror::release::{Release, find_superseded};
use overture_mirror::sync::{check_syncable, check_verifiable};

/// Mirror Overture Maps releases from the public bucket, or from one mirror to another.
#[derive(Parser)]
#[command(about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse().command).await {
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
        Commands::Sync(Transfer {
            release,
            source,
            mirror,
        }) => {
            let source = Source::open(&source.source)?;
            check_syncable(&release, &source.releases().await?, &mirror)?;
            let found = mirror::sync(&source, &release, &mirror, Display::Bars).await?;
            let count = |status| found.values().filter(|found| **found == status).count();
            println!(
                "{release}: {} already complete, {} copied, {} recopied",
                count(Local::Complete),
                count(Local::Missing),
                count(Local::Differs)
            );
        }
        Commands::Verify(Transfer {
            release,
            source,
            mirror,
        }) => {
            let source = Source::open(&source.source)?;
            check_verifiable(&release, &source.releases().await?, &mirror)?;
            let verification = mirror::verify(&source, &release, &mirror, Display::Bars).await?;
            for (location, state) in &verification.remote {
                if *state != Local::Complete {
                    println!("{state:?}  {location}");
                }
            }
            for path in &verification.local_only {
                println!("LocalOnly  {}", path.display());
            }
            let count = |status| {
                verification
                    .remote
                    .values()
                    .filter(|found| **found == status)
                    .count()
            };
            let incomplete = Incomplete {
                release: release.clone(),
                missing: count(Local::Missing),
                differing: count(Local::Differs),
                local_only: verification.local_only.len(),
            };
            if incomplete.missing + incomplete.differing + incomplete.local_only > 0 {
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
