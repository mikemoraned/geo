use std::{fs::File, io::BufWriter, path::PathBuf};

use clap::{command, Parser};
use geo::coord;
use geozero::{geojson::GeoJsonWriter, GeozeroGeometry};
use routing::stadia::{Profile, Server, StandardRouting};
use startup::env::load_secret;

/// Find routes in an area
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// stadiamaps server kind that we should talk to
    #[arg(short, long)]
    server: Server,

    /// profile
    #[arg(short, long)]
    profile: Profile,

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

    let queensferry = coord! { x: -3.409195, y: 55.992622 };
    let dalkeith = coord! { x: -3.066667, y: 55.866667 };

    let route = routing.find_route(&queensferry, &dalkeith, args.profile).await?;

    let fout = BufWriter::new(File::create(args.geojson)?);
    let mut gout = GeoJsonWriter::new(fout);
    geo::geometry::Geometry::LineString(route).process_geom(&mut gout)?;
    
    Ok(())
}