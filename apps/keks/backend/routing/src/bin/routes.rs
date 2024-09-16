use std::{fs::File, io::BufWriter, path::PathBuf};

use clap::{command, Parser};
use cli::progress::progress_bar;
use geo::{coord, GeometryCollection, Rect};
use geozero::{geojson::GeoJsonWriter, GeozeroGeometry};
use routing::stadia::{Profile, Server, StandardRouting};
use startup::env::load_secret;

/// Find routes in an area
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// stadiamaps server kind that we should talk to
    #[arg(long)]
    server: Server,

    /// profile
    #[arg(long)]
    profile: Profile,

    /// number of paths to generate
    #[arg(long)]
    paths: usize,

    /// output GeoJSON `.geojson` file
    #[arg(long)]
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
    let bounds = Rect::new(queensferry, dalkeith);
    let min = bounds.min();
    let max = bounds.max();
    let width = bounds.width();
    let height = bounds.height();
    let mut routes = vec![];
    let progress = progress_bar(args.paths as u64);
    for _ in 0..args.paths {
        let mut start = coord! {
            x: rand::random::<f64>() * width + min.x,
            y: rand::random::<f64>() * height + min.y,
        };
        let mut end = coord! {
            x: rand::random::<f64>() * width + min.x,
            y: rand::random::<f64>() * height + min.y,
        };
        if rand::random() {
            // clamp to run from top to bottom
            start.y = max.y;
            end.y = min.y;
        }
        else {
            // clamp to run from left to right
            start.x = min.x;
            end.x = max.x;
        }
        let route = routing.find_route(&start, &end, &args.profile).await?;
        routes.push(geo::geometry::Geometry::LineString(route));
        progress.inc(1);
    }
    let collection = GeometryCollection::new_from(routes);

    let fout = BufWriter::new(File::create(args.geojson)?);
    let mut gout = GeoJsonWriter::new(fout);
    geo::geometry::Geometry::GeometryCollection(collection).process_geom(&mut gout)?;
    
    Ok(())
}