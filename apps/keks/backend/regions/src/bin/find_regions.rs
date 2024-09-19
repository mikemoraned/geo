use std::{fs::File, io::BufReader, path::PathBuf};

use clap::{command, Parser};
use geo::{BoundingRect, Geometry};
use geozero::{geo_types::GeoWriter, geojson::GeoJsonReader, GeozeroDatasource};
use image::GrayImage;

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
        let width_px = (width * 10000.0).ceil() as u32;
        let height_px = (height * 10000.0).ceil() as u32;

        println!("Width: {} Height: {}", width, height);
        println!("Width px: {} Height px: {}", width_px, height_px);

        let image = GrayImage::new(width_px, height_px);
        image.save(args.png)?;
    }

    Ok(())
}

