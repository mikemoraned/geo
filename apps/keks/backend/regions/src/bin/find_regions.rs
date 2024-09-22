use std::{fs::File, io::{BufReader, Cursor}, path::PathBuf};

use clap::{command, Parser};
use geo::{BoundingRect, Geometry, GeometryCollection};
use geozero::{geo_types::GeoWriter, geojson::GeoJsonReader, GeozeroDatasource};
use image::{GrayImage, ImageReader};

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

fn draw(collection: &GeometryCollection) -> Result<GrayImage, Box<dyn std::error::Error>> {
    use tiny_skia::*;

    let bounds = collection.bounding_rect().unwrap();
    println!("Bounding rect: {:?}", bounds);
    let width = bounds.width() as f32;
    let height = bounds.height() as f32;
    let min_x = bounds.min().x as f32;
    let min_y = bounds.min().y as f32;

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

    let mut black = Paint::default();
    black.set_color_rgba8(0, 0, 0, 255);

    let mut white = Paint::default();
    white.set_color_rgba8(255, 255, 255, 255);

    let mut stroke = Stroke::default();
    stroke.width = 0.005 * width.min(height);

    // anything that is white is a border and anything that is black is a region

    // so, first make everything white by default
    pixmap.fill_rect(
        Rect::from_xywh(0.0, 0.0, width_px as f32, height_px as f32).ok_or("Failed to create rect")?,
        &white,
        Transform::identity(),
        None
    );

    // then draw any polygons in black as backgrounds for regions
    for geom in collection.iter() {
        if let Geometry::Polygon(poly) = geom {
            let mut pb = PathBuilder::new();
            poly.exterior().points().for_each(|p| {
                if pb.is_empty() {
                    pb.move_to(p.x() as f32, p.y() as f32);
                } else {
                    pb.line_to(p.x() as f32, p.y() as f32);
                }
            });
            pb.close();
            let path = pb.finish().ok_or("Failed to finish path")?;
            pixmap.fill_path(&path, &black, FillRule::EvenOdd, transform, None);
        }
    }

    // then draw linestrings as candidate borders
    for geom in collection.iter() {
        if let Geometry::LineString(line) = geom {
            let mut pb = PathBuilder::new();
            line.points().for_each(|p| {
                if pb.is_empty() {
                    pb.move_to(p.x() as f32, p.y() as f32);
                } else {
                    pb.line_to(p.x() as f32, p.y() as f32);
                }
            });
            let path = pb.finish().ok_or("Failed to finish path")?;
            pixmap.stroke_path(&path, &white, &stroke, transform, None);
        }
    }

    let png_bytes = pixmap.encode_png()?;
    let mut reader = ImageReader::new(Cursor::new(png_bytes));
    reader.set_format(image::ImageFormat::Png);
    let decoded = reader.decode()?;

    Ok(decoded.into_luma8())
}

