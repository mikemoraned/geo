use std::path::PathBuf;

use clap::Parser;
use geo_shell::{config::Config, tracing::setup_tracing_and_logging};
use tracing::info;

/// Find greenery in an area
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// config file defining the area
    #[arg(long)]
    config: PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_tracing_and_logging()?;   

    let args = Args::parse();

    let config: Config = Config::read_from_file(&args.config)?;

    info!("Read config: {config:?}");

    let overturemaps = config.overturemaps;
    let gers_id = overturemaps.gers_id;

    info!("Overturemaps Gers ID: {gers_id}");

    Ok(())
}
