use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use overture_mirror::mirror::{self, Bucket, Local};
use overture_mirror::release::{Release, find_superseded};
use overture_mirror::sync::{check_syncable, check_verifiable};

/// Mirror Overture Maps releases from the public bucket.
#[derive(Parser)]
#[command(about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List the releases the bucket serves, and mark each superseded one.
    Releases,
    /// Copy one release whole into the mirror, resuming from the files already there.
    Sync {
        release: Release,
        /// The directory holding one directory per mirrored release.
        #[arg(long)]
        mirror: PathBuf,
    },
    /// Check a mirrored release against the bucket, copying nothing. Fails if any file is off.
    Verify {
        release: Release,
        /// The directory holding one directory per mirrored release.
        #[arg(long)]
        mirror: PathBuf,
    },
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
    tracing_subscriber::fmt().with_target(false).init();
    match run(Args::parse().command).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

async fn run(command: Commands) -> Result<(), Box<dyn Error>> {
    let bucket = Bucket::overture()?;
    let served = bucket.releases().await?;
    match command {
        Commands::Releases => {
            let superseded = find_superseded(&served);
            for release in &served {
                match superseded.get(release) {
                    Some(latest) => println!("{release}  superseded by {latest}"),
                    None => println!("{release}"),
                }
            }
        }
        Commands::Sync { release, mirror } => {
            check_syncable(&release, &served, &mirror)?;
            let found = mirror::sync(&bucket, &release, &mirror).await?;
            let count = |status| found.values().filter(|found| **found == status).count();
            println!(
                "{release}: {} already complete, {} copied, {} recopied",
                count(Local::Complete),
                count(Local::Missing),
                count(Local::Differs)
            );
        }
        Commands::Verify { release, mirror } => {
            check_verifiable(&release, &served, &mirror)?;
            let verification = mirror::verify(&bucket, &release, &mirror).await?;
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
