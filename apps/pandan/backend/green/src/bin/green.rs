use std::path::PathBuf;

use clap::Parser;
use geo_overturemaps::{context::OvertureContext, io::save_as_geojson};
use geo_shell::{config::Config, tracing::setup_tracing_and_logging};
use tracing::{info, warn};

/// Find greenery in an area
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// config file defining the area
    #[arg(long)]
    config: PathBuf,

    /// Overturemaps Release base
    #[arg(long)]
    overturemaps: PathBuf,

    /// where to put the green data
    #[arg(long)]
    green: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_tracing_and_logging()?;   

    let args = Args::parse();
    info!("Parsed arguments: {args:?}");

    let config: Config = Config::read_from_file(&args.config)?;

    info!("Read config: {config:?}");

    let overturemaps = config.overturemaps;
    let gers_id = overturemaps.gers_id;

    info!("Overturemaps Gers ID: {gers_id}");

    let om = OvertureContext::load_from_release(args.overturemaps).await?;
    let geometry = om.find_geometry_by_id(&gers_id).await?;
    if let Some(region) = geometry {
        info!("Found region");
        if let Some(land_cover) = om.find_land_cover_in_region(&region).await? {
            info!("Found land cover ");
            save_as_geojson(&land_cover, &args.green)?;
            info!("Saved geometry to {:?}", args.green);
        } else {
            warn!("No land cover found in region");
        }
    } else {
        warn!("No geometry found with ID {gers_id}");
    }

    Ok(())
}
