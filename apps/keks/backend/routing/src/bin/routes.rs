use std::{fs::File, io::BufWriter, path::PathBuf};

use clap::{command, Parser};
use cli::progress::progress_bar;
use fast_poisson::Poisson2D;
use geo::{coord, Coord, GeometryCollection, Point, Rect};
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

    /// whether to save the points used as start and end
    #[arg(long, default_value_t = false)]
    save_points: bool,

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
    let progress = progress_bar(args.paths as u64);
    let starts = random_coords(&bounds, args.paths);
    let ends = random_coords(&bounds, args.paths);
    let paired : Vec<(Coord, Coord)> = starts.clone().into_iter().zip(ends.clone().into_iter()).collect();
    let mut geo = vec![];
    for (start, end) in paired {
        if args.save_points {
            geo.push(geo::geometry::Geometry::Point(Point::from(start.clone())));
            geo.push(geo::geometry::Geometry::Point(Point::from(end.clone())));
        }

        let route = routing.find_route(&start, &end, &args.profile).await?;
        geo.push(geo::geometry::Geometry::LineString(route));

        progress.inc(1);
    }
    let geo_collection = GeometryCollection::new_from(geo);

    let fout = BufWriter::new(File::create(args.geojson)?);
    let mut gout = GeoJsonWriter::new(fout);
    geo::geometry::Geometry::GeometryCollection(geo_collection).process_geom(&mut gout)?;
    
    Ok(())
}

fn random_coords(bounds: &Rect, n: usize) -> Vec<Coord> {
    let min = bounds.min();
    let width = bounds.width();
    let height = bounds.height();
    let radius = (width * height / n as f64).sqrt();
    let points : Vec<_> = Poisson2D::new()
        .with_dimensions([width, height], radius)
        .iter().take(n).collect();

    points
        .iter()
        .map(|[x_offset, y_offset]| coord! {
            x: x_offset + min.x,
            y: y_offset + min.y,
        })
        .collect()
}