use std::{fs::File, io::BufWriter, path::PathBuf};

use clap::{command, Parser};
use config::Config;
use geo::{BoundingRect, Geometry, GeometryCollection};
use geozero::{geojson::GeoJsonWriter, GeozeroGeometry};
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
    #[error("Unable to find bounds")]
    CannotFindBounds,
    #[error("Unable to find water")]
    CannotFindWater,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("{:?}", args);

    let config: Config = Config::read_from_file(&args.area)?;

    let gers_id = &config.overturemaps.gers_id;
    if let Some(om_base) = args.overturemaps.as_ref() {
        use overturemaps::overturemaps::OvertureMaps;
        let om = OvertureMaps::load_from_base(om_base.clone()).await?;

        if let Some(bounds) = om.find_geometry_by_id(gers_id).await? {
            println!("Bounds: {:?}", bounds);
            let rect_bounds = bounds.bounding_rect().ok_or(WaterError::CannotFindBounds)?;
            println!("Rect Bounds: {:?}", rect_bounds);
            let water = om.find_water_in_region(&rect_bounds).await?;
            println!("Water found: {:?}", water);
            save(&water, &args.water)?;
        }
    }

    Ok(())
}

fn save(geo: &geo::geometry::Geometry, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let collection = GeometryCollection::new_from(vec![geo.clone()]);

    let fout = BufWriter::new(File::create(path)?);
    let mut gout = GeoJsonWriter::new(fout);
    geo::geometry::Geometry::GeometryCollection(collection).process_geom(&mut gout)?;

    Ok(())
}
