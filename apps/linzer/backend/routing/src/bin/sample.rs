use std::{fs::File, io::BufWriter, path::PathBuf};

use clap::Parser;
use config::Config;
use fast_poisson::Poisson2D;
use geo::{
    coord, Area, BooleanOps, BoundingRect, Contains, Coord, Geometry, GeometryCollection,
    MultiPolygon, Point, Rect,
};
use geozero::{geojson::GeoJsonWriter, GeozeroGeometry};
use overturemaps::overturemaps::WaterHandling;
use rand::{RngCore, SeedableRng};
use routing::bounds;
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

    /// if the region has multiple polygons, choose the largest one
    #[arg(long, default_value_t = true)]
    choose_largest_polygon: bool,

    /// exclude points which are in water
    #[arg(long, default_value_t = true)]
    exclude_water: bool,

    /// number of points to generate
    #[arg(long)]
    paths: usize,

    /// seed for random number generator
    #[arg(long)]
    seed: u64,

    /// output GeoJSON `.geojson` file for bounds of region
    #[arg(long)]
    bounds: PathBuf,

    /// output GeoJSON `.geojson` file for mask used, which may exclude water
    #[arg(long)]
    mask: PathBuf,

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
    #[error("Could not find largest Polygon in MultiPolygon")]
    CannotFindLargestPolygon,
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

    let bounds = read_bounds(&args, &config).await?;
    save(&vec![bounds.clone()], &args.bounds)?;
    let masked = if args.exclude_water {
        println!("Excluding water from bounds");
        let water = read_water(&args, &config).await?;
        let masked = difference(&bounds, &water);
        println!(
            "size went from {} to {}",
            bounds.unsigned_area(),
            masked.unsigned_area()
        );
        masked
    } else {
        println!("Not excluding water from bounds");
        bounds.clone()
    };
    save(&vec![masked.clone()], &args.mask)?;

    let mut rng = rand::rngs::StdRng::seed_from_u64(args.seed);

    let mut starts = random_points(&masked, args.paths, rng.next_u64())?;
    let mut ends = random_points(&masked, args.paths, rng.next_u64())?;

    if starts.len() != ends.len() {
        println!(
            "Warning: number of starting points does not match number of ending points, {} != {}, correcting for now",
            starts.len(),
            ends.len()
        );
        let min_len = std::cmp::min(starts.len(), ends.len());
        starts = starts.into_iter().take(min_len).collect();
        ends = ends.into_iter().take(min_len).collect();
    }

    save(&starts, &args.starts)?;
    save(&ends, &args.ends)?;

    Ok(())
}

async fn read_bounds(args: &Args, config: &Config) -> Result<Geometry, Box<dyn std::error::Error>> {
    let gers_id = &config.overturemaps.gers_id;
    if let Some(om_base) = args.overturemaps.as_ref() {
        use overturemaps::overturemaps::OvertureMaps;
        let om = OvertureMaps::load_from_base(om_base.clone()).await?;
        Ok(bounds::read_bounds(gers_id, &om, args.choose_largest_polygon).await?)
    } else {
        Err(Box::new(SamplerError::MissingOvertureMapsBase))
    }
}

async fn read_water(args: &Args, config: &Config) -> Result<Geometry, Box<dyn std::error::Error>> {
    let gers_id = &config.overturemaps.gers_id;
    if let Some(om_base) = args.overturemaps.as_ref() {
        use overturemaps::overturemaps::OvertureMaps;
        let om = OvertureMaps::load_from_base(om_base.clone()).await?;
        if let Some(bounds) = om.find_geometry_by_id(gers_id).await? {
            let water = om
                .find_water_in_region(&bounds, WaterHandling::ClipToRegion)
                .await?;
            Ok(water)
        } else {
            Err(Box::new(SamplerError::CannotFindGersId))
        }
    } else {
        Err(Box::new(SamplerError::MissingOvertureMapsBase))
    }
}

fn difference(geo1: &Geometry<f64>, geo2: &Geometry<f64>) -> Geometry<f64> {
    match geo1 {
        Geometry::Polygon(poly1) => match geo2 {
            Geometry::Polygon(poly2) => {
                // println!("Difference between two polygons");
                Geometry::MultiPolygon(poly1.difference(poly2))
            }
            Geometry::MultiPolygon(multi2) => {
                // println!("Difference between polygon and multipolygon");
                Geometry::MultiPolygon(MultiPolygon::new(vec![poly1.clone()]).difference(multi2))
            }
            Geometry::GeometryCollection(GeometryCollection(parts)) => {
                // println!("Difference between polygon and geometry collection");
                let difference_on_parts = parts
                    .iter()
                    .fold(geo1.clone(), |acc, geo2| difference(&acc, geo2));
                difference_on_parts
            }
            _ => {
                // println!("Difference between polygon and non-polygon");
                Geometry::MultiPolygon(MultiPolygon::new(vec![poly1.clone()]))
            }
        },
        Geometry::MultiPolygon(multi1) => match geo2 {
            Geometry::Polygon(poly2) => {
                // println!("Difference between multipolygon and polygon");
                Geometry::MultiPolygon(multi1.difference(&MultiPolygon::new(vec![poly2.clone()])))
            }
            Geometry::MultiPolygon(multi2) => {
                // println!("Difference between two multipolygons");
                Geometry::MultiPolygon(multi1.difference(multi2))
            }
            _ => {
                // println!("Difference between multipolygon and non-polygon");
                Geometry::MultiPolygon(multi1.clone())
            }
        },

        _ => {
            // println!("Difference with non-polygon geometry, returning empty MultiPolygon");
            Geometry::MultiPolygon(MultiPolygon::new(vec![]))
        }
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

    // to ensure fair sampling across the whole area, we define a grid over the bounding box,
    // such that we want to have one random point per grid entry, and expect to have `n` points left over after
    // reducing to the points which overlap the bounds
    let filled_area = bounds.unsigned_area();
    let filled_fraction = filled_area / bounding_box.unsigned_area();
    let desired_grid_entries = (n as f64 / filled_fraction).ceil() as usize;
    let mut grid = Grid::new(&bounding_box, desired_grid_entries);

    // scale the radius between points based on the area being covered
    let square_area_per_point = filled_area / (n as f64);
    let side_length = square_area_per_point.sqrt();
    let diagonal_length = (2.0 * side_length.powi(2)).sqrt(); // hypotoneuse
    let radius = diagonal_length / 2.0;

    let mut sampler_seed = seed;
    let mut sample_iter = Poisson2D::new()
        .with_seed(sampler_seed)
        .with_dimensions([width, height], radius)
        .iter();

    while !grid.is_filled() {
        // println!(
        //     "{}% filled, {} remaining to fill\r",
        //     (grid.proportion_filled() * 100.0).round(),
        //     grid.count_remaining_to_fill
        // );
        if let Some([x_offset, y_offset]) = sample_iter.next() {
            let coord = coord! {
                x: x_offset + min.x,
                y: y_offset + min.y,
            };
            grid.add_coord(coord);
        } else {
            println!(
                "Ran out of random points to sample, but still need more, so recreating sampler with a new seed"
            );
            sampler_seed += 1;
            sample_iter = Poisson2D::new()
                .with_seed(sampler_seed)
                .with_dimensions([width, height], radius)
                .iter();
        }
    }

    let sample_coords = grid.into_coords();

    // go through all sample points, convert to coords, and find only those which overlap bounds
    let mut coords_within_bounds = vec![];
    for sample_coord in sample_coords.into_iter() {
        let sample_point = geo::geometry::Geometry::Point(Point::from(sample_coord));
        if bounds.contains(&sample_point) {
            coords_within_bounds.push(sample_point);
        }
    }

    let mut returned_n = n;
    if coords_within_bounds.len() < n {
        println!(
            "warn: Only found {} points within bounds, but needed {}, allowing to continue",
            coords_within_bounds.len(),
            n
        );
        returned_n = coords_within_bounds.len();
    }

    // need to then randomly sample from throughout the coords found
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let coords = coords_within_bounds
        .into_iter()
        .choose_multiple(&mut rng, returned_n);

    Ok(coords)
}

struct Grid {
    bounding_box: Rect,
    x_stride: f64,
    y_stride: f64,
    count_remaining_to_fill: usize,
    grid_size: usize,
    grid: Vec<Vec<bool>>,
    coords: Vec<Coord>,
}

impl Grid {
    pub fn new(bounding_box: &Rect, desired_grid_entries: usize) -> Self {
        // actual grid needs to be square so decide on a size which covers at least `desired_grid_entries`
        let grid_side_length = ((desired_grid_entries as f64).sqrt().ceil()) as usize;
        let x_stride = bounding_box.width() / (grid_side_length as f64);
        let y_stride = bounding_box.height() / (grid_side_length as f64);
        let grid_size = grid_side_length * grid_side_length;
        let count_remaining_to_fill = grid_side_length * grid_side_length;
        let grid = vec![vec![false; grid_side_length]; grid_side_length];
        let coords = vec![];
        Grid {
            bounding_box: bounding_box.clone(),
            x_stride,
            y_stride,
            count_remaining_to_fill,
            grid_size,
            grid,
            coords,
        }
    }

    pub fn proportion_filled(&self) -> f64 {
        (self.grid_size - self.count_remaining_to_fill) as f64 / self.grid_size as f64
    }

    pub fn is_filled(&self) -> bool {
        self.count_remaining_to_fill == 0
    }

    pub fn add_coord(&mut self, coord: Coord) {
        let x_index = ((coord.x - self.bounding_box.min().x) / self.x_stride).floor() as usize;
        let y_index = ((coord.y - self.bounding_box.min().y) / self.y_stride).floor() as usize;

        if x_index < self.grid.len() && y_index < self.grid[x_index].len() {
            if !self.grid[x_index][y_index] {
                self.grid[x_index][y_index] = true;
                self.count_remaining_to_fill -= 1;
                self.coords.push(coord);
            }
        }
    }

    pub fn into_coords(self) -> Vec<Coord> {
        self.coords
    }
}
