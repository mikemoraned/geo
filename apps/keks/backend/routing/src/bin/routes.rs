use std::{fs::File, io::BufWriter, path::PathBuf};

use clap::{command, Parser};
use geo::coord;
use geozero::{geojson::GeoJsonWriter, GeozeroGeometry};
use routing::stadia::{Profile, StandardRouting};
use startup::env::load_secret;

/// Find routes in an area
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// output GeoJSON `.geojson` file
    #[arg(short, long)]
    geojson: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let stadia_maps_api_key = load_secret("STADIA_MAPS_API_KEY")?;

    let routing = StandardRouting::new(
        &stadia_maps_api_key, routing::stadia::Server::Default
    )?;

    let edinburgh = coord! { x: -3.188267, y: 55.953251 };
    let queensferry = coord! { x: -3.409195, y: 55.992622 };

    let route = routing.find_route(&edinburgh, &queensferry, Profile::Auto).await?;

    let fout = BufWriter::new(File::create(args.geojson)?);
    let mut gout = GeoJsonWriter::new(fout);
    geo::geometry::Geometry::LineString(route).process_geom(&mut gout)?;
    
    Ok(())
}