use std::{collections::HashMap, fs::File, io::{BufReader, Cursor}, path::PathBuf};

use binpack2d::{bin_new, Dimension, Rectangle};
use clap::{command, Parser};
use conversion::projection::Projection;
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
    use tiny_skia::*;

    let max_x = rects.iter().map(|rect| rect.x() + rect.width()).max().unwrap();
    let max_y = rects.iter().map(|rect| rect.y() + rect.height()).max().unwrap();

    let mut pixmap = Pixmap::new(max_x as u32, max_y as u32).ok_or("Failed to create pixmap")?;

    let mut black = Paint::default();
    black.set_color(Color::BLACK);
    black.anti_alias = true;

    let mut white = Paint::default();
    white.set_color(Color::WHITE);
    white.anti_alias = true;

    let mut red = Paint::default();
    red.set_color_rgba8(255, 0, 0, 255);
    red.anti_alias = true;

    let mut stroke = Stroke::default();
    stroke.width = 0.0005 * (max_x as f32).min(max_y as f32);

    pixmap.fill_rect(
        Rect::from_xywh(0.0, 0.0, max_x as f32, max_y as f32).ok_or("Failed to create rect")?,
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
        pixmap.stroke_path(&path, &red, &stroke, Transform::identity(), None);

        let id = rect.id();
        let polygon = identified.get(&id).ok_or(format!("Failed to find geometry for id {}", id))?;
        let mut pb = PathBuilder::new();
        let bounds = polygon.bounding_rect().unwrap();
        polygon.exterior().points().for_each(|p| {
            let (x, y) = (
                (p.x() - bounds.min().x) * projection.scale_x + rect.x() as f64,
                (p.y() - bounds.min().y) * projection.scale_y + rect.y() as f64
            );
            if pb.is_empty() {
                pb.move_to(x as f32, y as f32);
            } else {
                pb.line_to(x as f32, y as f32);
            }
        });
        pb.close();
        let path = pb.finish().ok_or("Failed to finish path")?;
        pixmap.fill_path(&path, &white, FillRule::EvenOdd, Transform::identity(), None);
    }

    let png_bytes = pixmap.encode_png()?;
    let mut reader = ImageReader::new(Cursor::new(png_bytes));
    reader.set_format(image::ImageFormat::Png);
    let decoded = reader.decode()?;

    Ok(decoded.into_rgba8())
}
