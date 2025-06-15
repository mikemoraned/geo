use std::{fs::File, io::BufWriter, path::PathBuf};

use clap::Parser;
use config::Config;
use fast_poisson::Poisson2D;
use geo::{coord, BoundingRect, Contains, Geometry, GeometryCollection, Point, Rect};
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

    /// output GeoJSON `.geojson` file for bounds of region
    #[arg(long)]
    bounds: PathBuf,

    /// output GeoJSON `.geojson` file for starting points
    #[arg(long)]
    starts: PathBuf,

    /// output GeoJSON `.geojson` file for ending points
    #[arg(long)]
    ends: PathBuf,
}

#[derive(Error, Debug)]
pub enum SamplerError {
    #[error("OvertureMaps base dir required")]
    MissingOvertureMapsBase,
    #[error("Unable to find anything with that GERS Id")]
    CannotFindGersId,
    #[error("Geometry for GERS Id could be converted into bounding rect")]
    CannotCreateBoundingRect,
    #[error("Random sampling of area did not produce enough points")]
    CannotGetEnoughRandomPoints,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("{:?}", args);

    let config: Config = Config::read_from_file(&args.area)?;
    println!("Name: {}", config.bounds.name);

    let bounds = read_bounds(&args, &config).await?;
    println!("Bounds: {:?}", bounds);
    save(&vec![bounds.clone()], &args.bounds)?;

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
                Ok(geometry)
            } else {
                Err(Box::new(SamplerError::CannotFindGersId))
            }
        } else {
            Err(Box::new(SamplerError::MissingOvertureMapsBase))
        }
    } else {
        Ok(Geometry::Rect(Rect::new(
            config.bounds.point1,
            config.bounds.point2,
        )))
    }
}

fn save(
    geo: &Vec<geo::geometry::Geometry>,
    path: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let collection = GeometryCollection::new_from(geo.clone());

    let fout = BufWriter::new(File::create(path)?);
    let mut gout = GeoJsonWriter::new(fout);
    geo::geometry::Geometry::GeometryCollection(collection).process_geom(&mut gout)?;

    Ok(())
}

/// random_points generates `n` random points within the given bounds using a Poisson disk sampling algorithm.
fn random_points(
    bounds: &Geometry,
    n: usize,
    seed: u64,
) -> Result<Vec<Geometry>, Box<dyn std::error::Error>> {
    use geo::Area;
    use rand::seq::IteratorRandom;

    let bounding_box = bounds
        .bounding_rect()
        .ok_or(Box::new(SamplerError::CannotCreateBoundingRect))?;

    let min = bounding_box.min();
    let width = bounding_box.width();
    let height = bounding_box.height();

    // we sample more than `n` points to ensure we have enough valid points after filtering to the geometry bounds
    let filled_area = bounds.unsigned_area();
    let filled_fraction = filled_area / bounding_box.unsigned_area();
    let sample_n = (n as f64 / filled_fraction).ceil() as usize;
    println!(
        "Sampling {} points to get {} valid points, as filled area is {}% of rectangular bounds",
        sample_n,
        n,
        100.0 * filled_fraction
    );

    // also scale the radius between points based on the area being covered
    let square_area_per_point = filled_area / (n as f64);
    let side_length = square_area_per_point.sqrt();
    let diagonal_length = (2.0 * side_length.powi(2)).sqrt(); // hypotoneuse
    let radius = diagonal_length / 2.0;

    let sample_points: Vec<_> = Poisson2D::new()
        .with_seed(seed)
        .with_dimensions([width, height], radius)
        .iter()
        .take(sample_n)
        .collect();

    // go through all sample points, convert to coords, and find only those which overlap bounds
    let mut coords_within_bounds = vec![];
    for sample_point in sample_points.iter() {
        let [x_offset, y_offset] = sample_point;
        let sample_coord = coord! {
            x: x_offset + min.x,
            y: y_offset + min.y,
        };
        let sample_point = geo::geometry::Geometry::Point(Point::from(sample_coord));
        if bounds.contains(&sample_point) {
            coords_within_bounds.push(sample_point);
        }
    }

    if coords_within_bounds.len() < n {
        // TODO: could probably fix this by re-sampling with a larger `sample_n` but for now
        // for simplicity just fail
        return Err(Box::new(SamplerError::CannotGetEnoughRandomPoints));
    }

    // need to then randomly sample from throughout the coords found as the position is
    // ordered (if we just sample first `n` then we get a skewed distribution of the leftmost points)
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let coords = coords_within_bounds
        .into_iter()
        .choose_multiple(&mut rng, n);

    Ok(coords)
}
