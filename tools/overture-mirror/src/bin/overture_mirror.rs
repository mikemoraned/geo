use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use overture_mirror::aws;
use overture_mirror::release::{Release, find_superseded};
use overture_mirror::sync::check_syncable;

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
    /// Copy one release whole into the mirror.
    Sync {
        release: Release,
        /// The directory holding one directory per mirrored release.
        #[arg(long)]
        mirror: PathBuf,
    },
}

fn main() -> ExitCode {
    match run(Args::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Commands) -> Result<(), Box<dyn Error>> {
    let served = aws::served()?;
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
            aws::copy(&release, &mirror)?;
        }
    }
    Ok(())
}
