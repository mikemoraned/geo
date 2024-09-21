use std::{fs::File, io::{BufReader, Cursor}, path::PathBuf};

use clap::{command, Parser};
use geo::{BoundingRect, Geometry, GeometryCollection};
use geozero::{geo_types::GeoWriter, geojson::GeoJsonReader, GeozeroDatasource};
use image::{ImageReader, RgbaImage};

/// find regions in an area
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// input GeoJSON `.geojson` file
    #[arg(long)]
    geojson: PathBuf,

    /// output png image file
    #[arg(long)]
    png: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut file = BufReader::new(File::open(args.geojson)?);
    let mut reader = GeoJsonReader(&mut file);
    let mut writer = GeoWriter::new();
    reader.process_geom(&mut writer)?;

    if let Geometry::GeometryCollection(geoms) = writer.take_geometry().unwrap() {
        println!("Found {} geometries", geoms.len());

        let image = draw(&geoms)?;
        image.save(args.png)?;
    }

    Ok(())
}

fn draw(collection: &GeometryCollection) -> Result<RgbaImage, Box<dyn std::error::Error>> {
    use tiny_skia::*;

    let bounds = collection.bounding_rect().unwrap();
    println!("Bounding rect: {:?}", bounds);
    let width = bounds.width() as f32;
    let height = bounds.height() as f32;
    let min_x = bounds.min().x as f32;
    let min_y = bounds.min().y as f32;
    let max_x = bounds.max().x as f32;
    let max_y = bounds.max().y as f32;

    let scale = 10000.0 as f32;

    let scale_x = scale;
    let scale_y = scale;

    let offset_x = -1.0 * min_x;
    let offset_y = -1.0 * min_y;

    let width_px = (width * scale).ceil() as u32;
    let height_px = (height * scale).ceil() as u32;

    println!("Width: {} Height: {}", width, height);
    println!("Width px: {} Height px: {}", width_px, height_px);
    let mut pixmap = Pixmap::new(width_px, height_px).ok_or("Failed to create pixmap")?;

    let transform = Transform::from_translate(offset_x, offset_y).post_scale(scale_x, scale_y);

    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;

    let mut stroke = Stroke::default();
    stroke.width = 1.0;

    let path = {
        let mut pb = PathBuilder::new();
        pb.move_to(min_x, min_y);
        pb.line_to(max_x, min_y);
        pb.line_to(max_x, max_y);
        pb.line_to(min_x, max_y);
        pb.close();
        pb.finish().ok_or("Failed to finish path")?
    };
    println!("Path: {:?}", path);
    pixmap.stroke_path(&path, &paint, &stroke, transform, None);
    // pixmap.fill_path(&path, &paint, FillRule::EvenOdd, transform, None);

    let png_bytes = pixmap.encode_png()?;
    let mut reader = ImageReader::new(Cursor::new(png_bytes));
    reader.set_format(image::ImageFormat::Png);
    let decoded = reader.decode()?;

    Ok(decoded.into_rgba8())
}

