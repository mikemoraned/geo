use std::{fs::File, io::BufWriter, path::PathBuf};

use clap::{command, Parser};
use fast_poisson::Poisson2D;
use geo::{coord, Coord, GeometryCollection, Point, Rect};
use geozero::{geojson::GeoJsonWriter, GeozeroGeometry};
use rand::{RngCore, SeedableRng};
use serde::Deserialize;

/// Create sample points in area
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// config file defining the area
    #[arg(long)]
    area: PathBuf,

    /// number of points to generate
    #[arg(long)]
    paths: usize,

    /// seed for random number generator
    #[arg(long)]
    seed: u64,

    /// output GeoJSON `.geojson` file for starting points
    #[arg(long)]
    starts: PathBuf,

    /// output GeoJSON `.geojson` file for ending points
    #[arg(long)]
    ends: PathBuf,
}

#[derive(Deserialize, Debug)]
struct Config {
   bounds: Bounds
}

#[derive(Deserialize, Debug)]
struct Bounds {
    point1: Coord,
    point2: Coord,
    name: String
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("{:?}", args);

    let config_str = std::fs::read_to_string(&args.area)?;
    let config : Config = toml::from_str(&config_str)?;
    println!("Name: {}", config.bounds.name);
    
    let bounds = Rect::new(config.bounds.point1, config.bounds.point2); 

    let mut rng = rand::rngs::StdRng::seed_from_u64(args.seed);

    let starts = random_points(&bounds, args.paths, rng.next_u64())?;
    let ends = random_points(&bounds, args.paths, rng.next_u64())?;

    save(&starts, &args.starts)?;
    save(&ends, &args.ends)?;
    
    Ok(())
}

fn save(geo: &Vec<geo::geometry::Geometry>, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let collection = GeometryCollection::new_from(geo.clone());

    let fout = BufWriter::new(File::create(path)?);
    let mut gout = GeoJsonWriter::new(fout);
    geo::geometry::Geometry::GeometryCollection(collection).process_geom(&mut gout)?;

    Ok(())
}

fn random_points(bounds: &Rect, n: usize, seed: u64) -> Result<Vec<geo::geometry::Geometry>, Box<dyn std::error::Error>> {
    let min = bounds.min();
    let width = bounds.width();
    let height = bounds.height();
    let radius = (width * height / (n as f64)).sqrt() / 1.5;
    let points : Vec<_> = Poisson2D::new()
        .with_seed(seed)
        .with_dimensions([width, height], radius)
        .iter().take(n).collect();

    if points.len() != n {
        return Err(format!("expected {} points, got {}", n, points.len()).into());
    }

    Ok(points
        .iter()
        .map(|[x_offset, y_offset]| { 
            let coord = coord! {
                x: x_offset + min.x,
                y: y_offset + min.y,
            };
            geo::geometry::Geometry::Point(Point::from(coord))
        })
        .collect())
}