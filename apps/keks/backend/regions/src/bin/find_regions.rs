use std::{fs::File, io::BufReader, path::PathBuf};

use clap::{command, Parser};
use geo::{BoundingRect, Geometry};
use geozero::{geo_types::GeoWriter, geojson::GeoJsonReader, GeozeroDatasource};
use image::RgbaImage;

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
    use flo_canvas::*;
    use flo_render_canvas::*;
    use futures::stream;

    let mut context = initialize_offscreen_rendering().map_err(|e| format!("failed to get context: {:?}", e))?;

    let mut drawing = vec![];
    drawing.clear_canvas(Color::Rgba(0.0, 0.0, 0.0, 0.0));
    drawing.canvas_height(height_px as f32);
    drawing.center_region(0.0, 0.0, width_px as f32, height_px as f32);

    drawing.new_path();
    drawing.move_to(0.0, 0.0);
    drawing.line_to(width_px as f32, 0.0);
    drawing.line_to(width_px as f32, height_px as f32);
    drawing.line_to(0.0, height_px as f32);
    drawing.close_path();

    drawing.fill_color(Color::Rgba(0.0, 0.0, 0.0, 1.0));
    drawing.fill();

    drawing.new_path();
    drawing.move_to(0.0, 0.0);
    drawing.line_to(width_px as f32 / 4.0, 0.0);
    drawing.line_to(width_px as f32 / 4.0, height_px as f32 / 4.0);
    drawing.line_to(0.0, height_px as f32 / 4.0);
    drawing.close_path();

    drawing.fill_color(Color::Rgba(1.0, 1.0, 1.0, 1.0));
    drawing.fill();

    let rendered = render_canvas_offscreen(&mut context, width_px, height_px, 1.0, stream::iter(drawing)).await;

    let image = RgbaImage::from_vec(width_px as u32, height_px as u32, rendered).ok_or("failed to create image from canvas")?;

    Ok(image)
}

