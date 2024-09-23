use std::{fs::File, io::BufReader, path::PathBuf};

use binpack2d::{bin_new, Dimension};
use clap::{command, Parser};
use conversion::projection::Projection;
use geo::{BoundingRect, Geometry, GeometryCollection};
use geozero::{geo_types::GeoWriter, geojson::GeoJsonReader, GeozeroDatasource};

#[derive(clap:: ValueEnum, Clone, Debug)]
enum BinType {
    /// use the guillotine bin packing algorithm
    Guillotine,
    /// use the maxrects bin packing algorithm
    MaxRects
}

impl Into<binpack2d::BinType> for BinType {
    fn into(self) -> binpack2d::BinType {
        match self {
            BinType::Guillotine => binpack2d::BinType::Guillotine,
            BinType::MaxRects => binpack2d::BinType::MaxRects,
        }
    }
}

/// find regions in an area
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// input GeoJSON `.geojson` file representing the regions found
    #[arg(long)]
    regions: PathBuf,

    /// type of bin packing algorithm to use
    #[arg(long)]
    bin_type: BinType,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("{:?}", args);

    let mut file = BufReader::new(File::open(args.regions)?);
    let mut reader = GeoJsonReader(&mut file);
    let mut writer = GeoWriter::new();
    reader.process_geom(&mut writer)?;

    if let Geometry::GeometryCollection(collection) = writer.take_geometry().unwrap() {
        println!("Found {} geometries", collection.len());

        let (dimensions, projection) = create_dimensions(&collection);

        let bounds = collection.bounding_rect().unwrap();
        let (max_x, max_y) = (
            (bounds.width() * projection.scale_x) as i32, 
            (bounds.height() * projection.scale_y) as i32
        );
        let mut bin = bin_new(args.bin_type.into(), max_x * 2, max_y * 2);
        let (inserted, rejected) = bin.insert_list(&dimensions);
        println!("Inserted: {} Rejected: {}", inserted.len(), rejected.len());
    }

    Ok(())
}

fn create_dimensions(collection: &GeometryCollection) -> (Vec<Dimension>, Projection) {
    let scale = 10000.0;
    let projection = Projection::from_geo_bounding_box_to_scaled_space(collection.bounding_rect().unwrap(), scale);

    let mut dimensions = vec![];
    for (id, geometry) in collection.iter().enumerate() {
        if let Geometry::Polygon(polygon) = geometry {
            let bounds = polygon.bounding_rect().unwrap();
            let dimension = Dimension::with_id(
                id as isize, 
                (bounds.width() * projection.scale_x) as i32, 
                (bounds.height() * projection.scale_y) as i32, 
                0);
            dimensions.push(dimension);
        }
    }
    (dimensions, projection)
}
