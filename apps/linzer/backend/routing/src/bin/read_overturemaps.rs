use std::collections::HashMap;
use std::fs;
use std::fs::File;
use clap::Parser;
use geoarrow::io::parquet::GeoParquetDatasetMetadata;
use parquet::arrow::arrow_reader::{ArrowReaderMetadata, ArrowReaderOptions};

/// Load some data from overturemaps
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// input partitioned root directory
    #[arg(long)]
    overturemaps: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("{:?}", args);

    let table_path = format!("{}/theme=divisions/type=division_area/", &args.overturemaps);

    // now try doing something with geoarrow
    let paths = fs::read_dir(&table_path.clone()).unwrap();
    let mut map = HashMap::new();
    for path in paths {
        // let file = path.unwrap().path().file_name().unwrap().to_str().unwrap().to_string();
        let name = path.unwrap().path();
        let file = File::open(name.clone()).unwrap();
        println!("file: {file:?}");
        let metadata = ArrowReaderMetadata::load(&file, ArrowReaderOptions::new())?;
        map.insert(String::from(name.as_path().to_str().unwrap()), metadata);
    }
    let dataset = GeoParquetDatasetMetadata::from_files(map)?;

    Ok(())
}
