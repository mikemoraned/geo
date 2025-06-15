use std::{
    fs::File,
    io::{BufReader, BufWriter},
    path::PathBuf,
};

use clap::{command, Parser};
use cli::progress::progress_bar;
use geo::{BoundingRect, Coord, Geometry, GeometryCollection, LineString, Point, Rect};
use geozero::{
    geo_types::GeoWriter,
    geojson::{GeoJsonReader, GeoJsonWriter},
    GeozeroDatasource, GeozeroGeometry,
};
use routing::stadia::{Profile, Server, StandardRouting};
use startup::env::load_secret;

/// Find routes in an area
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// stadiamaps server kind that we should talk to
    #[arg(long, default_value_t = Server::default())]
    server: Server,

    /// how many times it should retry a routing request before giving up
    #[arg(long, default_value = "2")]
    max_retries: u32,

    /// profile
    #[arg(long)]
    profile: Profile,

    /// GeoJSON `.geojson` file for starting points
    #[arg(long)]
    starts: PathBuf,

    /// GeoJSON `.geojson` file for ending points
    #[arg(long)]
    ends: PathBuf,

    /// whether to save the bounds of the area
    #[arg(long, default_value_t = true)]
    save_bounds: bool,

    /// output GeoJSON `.geojson` file
    #[arg(long)]
    geojson: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("{:?}", args);

    let stadia_maps_api_key = load_secret("STADIA_MAPS_API_KEY")?;

    let routing = StandardRouting::new(&stadia_maps_api_key, routing::stadia::Server::Default)?;

    let mut geo = vec![];

    let starts = read_points(&args.starts)?;
    let ends = read_points(&args.ends)?;
    if starts.len() != ends.len() {
        return Err(format!(
            "number of starting points must match number of ending points, {} != {}",
            starts.len(),
            ends.len()
        )
        .into());
    }
    if starts.is_empty() {
        return Err("no starting points found".into());
    }

    let bounds = bounds(&starts, &ends)?;
    if args.save_bounds {
        geo.push(geo::geometry::Geometry::Rect(bounds));
    }

    let paired: Vec<(Point, Point)> = starts
        .clone()
        .into_iter()
        .zip(ends.clone().into_iter())
        .collect();
    let progress = progress_bar(paired.len() as u64);

    for (Point(start), Point(end)) in paired {
        match find_route(&routing, &start, &end, &args.profile, args.max_retries).await {
            Ok(route) => {
                geo.push(geo::geometry::Geometry::LineString(route));
            }
            Err(e) => {
                println!(
                    "Error finding route from {:?} to {:?}: {}, so will skip",
                    start, end, e
                );
            }
        }

        progress.inc(1);
    }
    let geo_collection = GeometryCollection::new_from(geo);

    let fout = BufWriter::new(File::create(args.geojson)?);
    let mut gout = GeoJsonWriter::new(fout);
    geo::geometry::Geometry::GeometryCollection(geo_collection).process_geom(&mut gout)?;

    Ok(())
}

async fn find_route(
    routing: &StandardRouting,
    start: &Coord,
    end: &Coord,
    profile: &Profile,
    max_retries: u32,
) -> Result<LineString, Box<dyn std::error::Error>> {
    let mut retries_remaining = max_retries;
    while retries_remaining > 0 {
        retries_remaining -= 1;
        match routing.find_route(&start, &end, &profile).await {
            Ok(route) => return Ok(route),
            Err(e) => {
                println!(
                    "Error whilst getting route: {:?}, start: {:?}, end: {:?}, profile: {:?}",
                    e, start, end, profile
                );
            }
        }
    }
    return Err(format!("Failed to find route after {} retries", max_retries).into());
}

fn bounds(
    starts: &Vec<geo::Point>,
    ends: &Vec<geo::Point>,
) -> Result<Rect, Box<dyn std::error::Error>> {
    let points: Vec<_> = starts
        .iter()
        .chain(ends.iter())
        .map(|p| Geometry::Point(p.clone()))
        .collect();
    let combined = GeometryCollection::new_from(points.clone());
    combined
        .bounding_rect()
        .ok_or("failed to calculate bounds".into())
}

fn read_points(path: &PathBuf) -> Result<Vec<geo::Point>, Box<dyn std::error::Error>> {
    let mut file = BufReader::new(File::open(path)?);
    let mut reader = GeoJsonReader(&mut file);
    let mut writer = GeoWriter::new();
    reader.process_geom(&mut writer)?;

    let geometry = writer
        .take_geometry()
        .ok_or(format!("failed read points from {:?}", path))?;

    if let Geometry::GeometryCollection(collection) = geometry {
        let mut points = vec![];
        for geometry in collection.iter() {
            if let Geometry::Point(point) = geometry {
                points.push(point.clone());
            }
        }
        Ok(points)
    } else {
        Ok(vec![])
    }
}
