use std::{fs::File, io::BufWriter, path::PathBuf};

use clap::{Parser, command};
use geo::GeometryCollection;
use geo_shell::config::Config;
use geozero::{GeozeroGeometry, geojson::GeoJsonWriter};
use overturemaps::overturemaps::WaterHandling;
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

    /// water handling
    #[arg(long, value_enum)]
    handling: WaterHandling,
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
            let water = om.find_water_in_region(&bounds, args.handling).await?;
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
