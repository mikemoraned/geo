use std::path::PathBuf;

use clap::{command, Parser};
use config::Config;
use geo::Geometry;
use thiserror::Error;

/// Find routes in an area
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// config file defining the area
    #[arg(long)]
    area: PathBuf,

    /// base location for OvertureMaps data
    #[arg(long)]
    overturemaps: Option<String>,

    /// output GeoJSON `.geojson` file representing the water found
    #[arg(long)]
    water: PathBuf,
}

#[derive(Error, Debug)]
pub enum WaterError {
    #[error("OvertureMaps base dir required")]
    MissingOvertureMapsBase,
    #[error("Unable to find anything with that GERS Id")]
    CannotFindGersId,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("{:?}", args);

    let config: Config = Config::read_from_file(&args.area)?;

    let bounds = read_bounds(&args, &config).await?;

    Ok(())
}

async fn read_bounds(args: &Args, config: &Config) -> Result<Geometry, Box<dyn std::error::Error>> {
    println!("Using overture maps");
    let gers_id = &config.overturemaps.gers_id;
    if let Some(om_base) = args.overturemaps.as_ref() {
        use overturemaps::overturemaps::OvertureMaps;
        let om = OvertureMaps::load_from_base(om_base.clone()).await?;

        if let Some(geometry) = om.find_geometry_by_id(gers_id).await? {
            Ok(geometry)
        } else {
            return Err(Box::new(WaterError::CannotFindGersId));
        }
    } else {
        Err(Box::new(WaterError::MissingOvertureMapsBase))
    }
}
