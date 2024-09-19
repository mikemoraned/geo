use std::{fs::File, io::BufReader, path::PathBuf};

use clap::{command, Parser};
use geo::{geometry, Geometry};
use geozero::{geo_types::GeoWriter, geojson::GeoJsonReader, GeozeroDatasource};

/// find regions in an area
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// input GeoJSON `.geojson` file
    #[arg(long)]
    geojson: PathBuf,
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

        for geom in geoms {
            if let geometry::Geometry::LineString(ls) = geom {
                println!("Found LineString with {} points", ls.0.len());
            }
        }
    }

    Ok(())
}

