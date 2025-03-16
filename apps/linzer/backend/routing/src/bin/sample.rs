use std::{fs::File, io::BufWriter, path::PathBuf};

use clap::Parser;
use config::Config;
use fast_poisson::Poisson2D;
use geo::{coord, BoundingRect, Geometry, GeometryCollection, Point, Rect};
use geozero::{geojson::GeoJsonWriter, GeozeroGeometry};
use rand::{RngCore, SeedableRng};
use thiserror::Error;

/// Create sample points in area
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// config file defining the area
    #[arg(long)]
    area: PathBuf,

    /// base location for OvertureMaps data
    #[arg(long)]
    overturemaps: Option<String>,

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

#[derive(Error, Debug)]
pub enum SamplerError {
    #[error("overture maps base dir required")]
    MissingOvertureMapsBase,
    #[error("unable to find anything with that GERS Id")]
    CannotFindGersId,
    #[error("Geometry for GERS Id could be converted into bounding rect")]
    CannotCreateBoundingRect,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("{:?}", args);

    let config : Config = Config::read_from_file(&args.area)?;
    println!("Name: {}", config.bounds.name);
    
    let bounds = read_bounds(&args, &config).await?;
    println!("Bounds: {:?}", bounds);

    let mut rng = rand::rngs::StdRng::seed_from_u64(args.seed);

    let starts = random_points(&bounds, args.paths, rng.next_u64())?;
    let ends = random_points(&bounds, args.paths, rng.next_u64())?;

    save(&starts, &args.starts)?;
    save(&ends, &args.ends)?;
    
    Ok(())
}

async fn read_bounds(args: &Args, config: &Config) -> Result<Geometry, Box<dyn std::error::Error>> {
    if let Some(om) = config.overturemaps.as_ref() {
        println!("Using overture maps");
        let gers_id = &om.gers_id;
        if let Some(om_base) = args.overturemaps.as_ref() {
            use overturemaps::overturemaps::OvertureMaps;
            let om = OvertureMaps::load_from_base(om_base.clone()).await?;
            if let Some(geometry) = om.find_geometry_by_id(gers_id).await? {
                if let Some(rect) = geometry.bounding_rect() {
                    Ok(Geometry::Rect(rect))
                }
                else {
                    Err(Box::new(SamplerError::CannotCreateBoundingRect))
                }
            }
            else {
                Err(Box::new(SamplerError::CannotFindGersId))
            }
        }
        else {
            Err(Box::new(SamplerError::MissingOvertureMapsBase))
        }
    }
    else {
        Ok(Geometry::Rect(Rect::new(config.bounds.point1, config.bounds.point2)))
    }
}

fn save(geo: &Vec<geo::geometry::Geometry>, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let collection = GeometryCollection::new_from(geo.clone());

    let fout = BufWriter::new(File::create(path)?);
    let mut gout = GeoJsonWriter::new(fout);
    geo::geometry::Geometry::GeometryCollection(collection).process_geom(&mut gout)?;

    Ok(())
}

fn random_points(bounds: &Geometry, n: usize, seed: u64) -> Result<Vec<Geometry>, Box<dyn std::error::Error>> {
    let bounding_box = bounds.bounding_rect().ok_or(Box::new(SamplerError::CannotCreateBoundingRect))?;
    let min = bounding_box.min();
    let width = bounding_box.width();
    let height = bounding_box.height();
    let radius = (width * height / (n as f64)).sqrt() / 1.5;
    let points : Vec<_> = Poisson2D::new()
        .with_seed(seed)
        .with_dimensions([width, height], radius)
        .iter().take(n).collect();

    let coords = points
        .iter()
        .map(|[x_offset, y_offset]| {
            let coord = coord! {
                x: x_offset + min.x,
                y: y_offset + min.y,
            };
            geo::geometry::Geometry::Point(Point::from(coord))
        })
        .collect();

    if points.len() != n {
        return Err(format!("expected {} points, got {}", n, points.len()).into());
    }

    Ok(coords)
}