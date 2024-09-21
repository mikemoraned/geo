use std::{fs::File, io::BufReader, path::PathBuf};

use clap::{command, Parser};
use geo::{BoundingRect, Geometry, Rect};
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

        let image = draw(&bounds, width_px, height_px).await?;
        image.save(args.png)?;
    }

    Ok(())
}

async fn draw(bounds: &Rect, width_px: usize, height_px: usize) -> Result<RgbaImage, Box<dyn std::error::Error>> {
    use flo_canvas::*;
    use flo_render_canvas::*;
    use futures::stream;

    let mut context = initialize_offscreen_rendering().map_err(|e| format!("failed to get context: {:?}", e))?;

    let width = bounds.width() as f32;
    let height = bounds.height() as f32;
    let min_x = bounds.min().x as f32;
    let min_y = bounds.min().y as f32;
    let max_x = bounds.max().x as f32;
    let max_y = bounds.max().y as f32;

    let mut drawing = vec![];
    drawing.clear_canvas(Color::Rgba(0.0, 0.0, 0.0, 0.0));
    // drawing.canvas_height(height);
    drawing.center_region(min_x, min_y, max_x, max_y);

    drawing.new_path();
    drawing.move_to(min_x, min_y);
    drawing.line_to(max_x, min_y);
    drawing.line_to(max_x, max_y);
    drawing.line_to(min_x, max_y);
    drawing.close_path();

    drawing.fill_color(Color::Rgba(0.0, 0.0, 0.0, 1.0));
    drawing.fill();

    let rendered = render_canvas_offscreen(&mut context, width_px, height_px, width_px as f32 / width, stream::iter(drawing)).await;

    let image = RgbaImage::from_vec(width_px as u32, height_px as u32, rendered).ok_or("failed to create image from canvas")?;

    Ok(image)
}

