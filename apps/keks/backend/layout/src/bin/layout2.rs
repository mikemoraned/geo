use std::{collections::HashMap, fs::File, io::{BufReader, Cursor}, path::PathBuf};

use binpack2d::{bin_new, Dimension, Rectangle};
use clap::{command, Parser};
use conversion::projection::Projection;
use display::PixmapLayout;
use geo::{BoundingRect, Geometry, GeometryCollection, Polygon};
use geozero::{geo_types::GeoWriter, geojson::GeoJsonReader, GeozeroDatasource};
use image::{ImageReader, RgbaImage};

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

        let (dimensions, projection, identified) = create_dimensions(&collection);

        let bounds = collection.bounding_rect().unwrap();
        let (width, height) = (
            (bounds.width() * projection.scale_x) as i32 * 2, 
            (bounds.height() * projection.scale_y) as i32 * 2
        );
        let mut bin = bin_new(args.bin_type.into(), width, height);
        let (inserted, rejected) = bin.insert_list(&dimensions);
        println!("Inserted: {} Rejected: {}", inserted.len(), rejected.len());

        let image = draw_layout(&inserted, &projection, &identified)?;
        image.save(args.layout)?;
    }

    Ok(())
}

fn create_dimensions(collection: &GeometryCollection) -> (Vec<Dimension>, Projection, HashMap<isize, &Polygon>) {
    let scale = 10000.0;
    let projection = Projection::from_geo_bounding_box_to_scaled_space(collection.bounding_rect().unwrap(), scale);
    let mut identified = HashMap::new();

    let mut dimensions = vec![];
    for (index, geometry) in collection.iter().enumerate() {
        if let Geometry::Polygon(polygon) = geometry {
            let bounds = polygon.bounding_rect().unwrap();
            let id = index as isize;
            let dimension = Dimension::with_id(
                id, 
                (bounds.width() * projection.scale_x) as i32, 
                (bounds.height() * projection.scale_y) as i32, 
                0);
            dimensions.push(dimension);
            identified.insert(id, polygon);
        }
    }
    (dimensions, projection, identified)
}

fn draw_layout(rects: &Vec<Rectangle>, projection: &Projection, identified: &HashMap<isize, &Polygon>) -> Result<RgbaImage, Box<dyn std::error::Error>> {
    let max_x = rects.iter().map(|rect| rect.x() + rect.width()).max().unwrap();
    let max_y = rects.iter().map(|rect| rect.y() + rect.height()).max().unwrap();

    let pixmap_builder = PixmapLayout::new(max_x as u32, max_y as u32)?;

    let png_bytes = pixmap_builder.encode_png()?;
    let mut reader = ImageReader::new(Cursor::new(png_bytes));
    reader.set_format(image::ImageFormat::Png);
    let decoded = reader.decode()?;

    Ok(decoded.into_rgba8())
}
