use std::{fs::File, io::BufReader, path::PathBuf};

use binpack2d::{bin_new, Dimension, Rectangle};
use clap::{command, Parser};
use conversion::projection::Projection;
use geo::{BoundingRect, Geometry, GeometryCollection};
use geozero::{geo_types::GeoWriter, geojson::GeoJsonReader, GeozeroDatasource};
use tiny_skia::Pixmap;

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

        let (dimensions, projection) = create_dimensions(&collection);

        let bounds = collection.bounding_rect().unwrap();
        let (width, height) = (
            (bounds.width() * projection.scale_x) as i32 * 2, 
            (bounds.height() * projection.scale_y) as i32 * 2
        );
        let mut bin = bin_new(args.bin_type.into(), width, height);
        let (inserted, rejected) = bin.insert_list(&dimensions);
        println!("Inserted: {} Rejected: {}", inserted.len(), rejected.len());

        let pixmap = draw_layout(&inserted, width as u32, height as u32)?;
        pixmap.save_png(args.layout)?;
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

fn draw_layout(rects: &Vec<Rectangle>, width: u32, height: u32) -> Result<Pixmap, Box<dyn std::error::Error>> {
    use tiny_skia::*;

    let max_x = rects.iter().map(|rect| rect.x() + rect.width()).max().unwrap() as u32;
    let max_y = rects.iter().map(|rect| rect.y() + rect.height()).max().unwrap() as u32;

    let mut pixmap = Pixmap::new(max_x, max_y).ok_or("Failed to create pixmap")?;

    let mut black = Paint::default();
    black.set_color(Color::BLACK);
    black.anti_alias = true;

    let mut white = Paint::default();
    white.set_color(Color::WHITE);
    white.anti_alias = true;

    let mut stroke = Stroke::default();
    stroke.width = 0.005 * (width as f32).min(height as f32);

    pixmap.fill_rect(
        Rect::from_xywh(0.0, 0.0, width as f32, height as f32).ok_or("Failed to create rect")?,
        &black,
        Transform::identity(),
        None
    );

    for rect in rects.iter() {
        let mut pb = PathBuilder::new();
        pb.move_to(rect.x() as f32, rect.y() as f32);
        pb.line_to(rect.x() as f32 + rect.width() as f32, rect.y() as f32);
        pb.line_to(rect.x() as f32 + rect.width() as f32, rect.y() as f32 + rect.height() as f32);
        pb.line_to(rect.x() as f32, rect.y() as f32 + rect.height() as f32);
        pb.close();
        let path = pb.finish().ok_or("Failed to finish path")?;
        pixmap.stroke_path(&path, &white, &stroke, Transform::identity(), None);
    }

    Ok(pixmap)
}
