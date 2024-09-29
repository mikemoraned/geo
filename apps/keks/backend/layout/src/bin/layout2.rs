use std::{collections::HashMap, fs::File, io::{BufReader, Cursor}, path::PathBuf};

use binpack2d::{bin_new, Dimension, Rectangle};
use clap::{command, Parser};
use conversion::projection::Projection;
use display::PixmapLayout;
use geo::{BoundingRect, Geometry, GeometryCollection, Polygon};
use geozero::{geo_types::GeoWriter, geojson::GeoJsonReader, GeozeroDatasource};
use image::{ImageReader, RgbaImage};
use layout::{Region, Regions};
use nalgebra::Point2;

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

    /// output image file representing the layout
    #[arg(long)]
    layout: PathBuf,
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

        let regions = create_regions(&collection);
        let dimensions = create_dimensions(&regions);
    }

    Ok(())
}

fn create_dimensions(regions: &Regions<f32>) -> Vec<Dimension> {
    let mut dimensions = vec![];
    for (id,region) in regions.iter() {
        // dimensions.push(Dimension::with_id(*id as isize, region.width(), region.height(), 0))
    }
    dimensions
}

fn create_regions(collection: &GeometryCollection) -> Regions<f32> {
    let scale = 10000.0;
    let projection = Projection::from_geo_bounding_box_to_scaled_space(collection.bounding_rect().unwrap(), scale);
    let mut regions = HashMap::new();

    for (index, geometry) in collection.iter().enumerate() {
        if let Geometry::Polygon(polygon) = geometry {
            let mut points = vec![];
            let bounds = polygon.bounding_rect().unwrap();
            polygon.exterior().points().for_each(|p| {
                let (normalised_x, normalised_y) = (
                    (p.x() - bounds.min().x) * projection.scale_x,
                    (p.y() - bounds.min().y) * projection.scale_y
                );
                points.push(Point2::new(normalised_x as f32, normalised_y as f32));
            });
            regions.insert(index, Region::new(points));
        }
    }

    Regions::new(regions)
}
