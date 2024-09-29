use std::{fs::File, io::BufWriter, path::PathBuf};

use clap::{command, Parser};
use fast_poisson::Poisson2D;
use geo::{coord, GeometryCollection, Point, Rect};
use geozero::{geojson::GeoJsonWriter, GeozeroGeometry};

/// Create sample points in area
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// number of points to generate
    #[arg(long)]
    paths: usize,

    /// output GeoJSON `.geojson` file for starting points
    #[arg(long)]
    starts: PathBuf,

    /// output GeoJSON `.geojson` file for ending points
    #[arg(long)]
    ends: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("{:?}", args);
    
    let queensferry = coord! { x: -3.409195, y: 55.992622 };
    let dalkeith = coord! { x: -3.066667, y: 55.866667 };
    let bounds = Rect::new(queensferry, dalkeith);

    let starts = random_points(&bounds, args.paths);
    let ends = random_points(&bounds, args.paths);

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

fn random_points(bounds: &Rect, n: usize) -> Vec<geo::geometry::Geometry> {
    let min = bounds.min();
    let width = bounds.width();
    let height = bounds.height();
    let radius = (width * height / n as f64).sqrt();
    let points : Vec<_> = Poisson2D::new()
        .with_dimensions([width, height], radius)
        .iter().take(n).collect();

    points
        .iter()
        .map(|[x_offset, y_offset]| { 
            let coord = coord! {
                x: x_offset + min.x,
                y: y_offset + min.y,
            };
            geo::geometry::Geometry::Point(Point::from(coord))
        })
        .collect()
}