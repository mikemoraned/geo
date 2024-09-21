use std::{fs::File, io::{BufReader, Cursor}, path::PathBuf};

use clap::{command, Parser};
use geo::{BoundingRect, Geometry, Rect};
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
        let bounds = geoms.bounding_rect().unwrap();
        println!("Bounding rect: {:?}", bounds);
        let width = bounds.width();
        let height = bounds.height();
        let width_px = (width * 10000.0).ceil() as usize;
        let height_px = (height * 10000.0).ceil() as usize;

        println!("Width: {} Height: {}", width, height);
        println!("Width px: {} Height px: {}", width_px, height_px);

        let image = draw(width_px, height_px).await?;
        image.save(args.png)?;
    }

    Ok(())
}

async fn draw(width_px: usize, height_px: usize) -> Result<RgbaImage, Box<dyn std::error::Error>> {
    use tiny_skia::*;

    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;

    // let min_x = bounds.min().x as f32;
    // let min_y = bounds.min().y as f32;
    // let max_x = bounds.max().x as f32;
    // let max_y = bounds.max().y as f32;

    let min_x = 0.0;
    let min_y = 0.0;
    let max_x = width_px as f32;
    let max_y = height_px as f32;

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

    let mut stroke = Stroke::default();
    stroke.width = width_px as f32 / 100.0;

    let mut pixmap = Pixmap::new(width_px as u32, height_px as u32).ok_or("Failed to create pixmap")?;
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);

    let png_bytes = pixmap.encode_png()?;
    let mut reader = ImageReader::new(Cursor::new(png_bytes));
    reader.set_format(image::ImageFormat::Png);
    let decoded = reader.decode()?;

    Ok(decoded.into_rgba8())
}

