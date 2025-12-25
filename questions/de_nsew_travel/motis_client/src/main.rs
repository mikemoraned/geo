use arrow::array::AsArray;
use arrow::datatypes::{DataType, Float64Type};
use chrono::{DateTime, Utc};
use clap::Parser;
use datafusion::prelude::*;
use motis_openapi_progenitor::types::Mode;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the parquet file containing origin and destination data
    #[arg(short, long)]
    parquet_file: String,

    /// Departure time as ISO 8601 string (e.g., "2026-01-10T09:00:00Z")
    #[arg(short = 't', long, value_parser = parse_datetime)]
    departure_time: DateTime<Utc>,
}

fn parse_datetime(s: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    s.parse::<DateTime<Utc>>()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Create DataFusion context and read parquet file
    let ctx = SessionContext::new();
    let df = ctx
        .read_parquet(&args.parquet_file, ParquetReadOptions::default())
        .await?
        .with_column("id_origin", cast(col("id_origin"), DataType::Utf8))?
        .with_column("id_dest", cast(col("id_dest"), DataType::Utf8))?;

    // Collect all data into memory
    let batches = df.collect().await?;

    // Create MOTIS client
    let client = motis_openapi_progenitor::Client::new("http://localhost:8080");
    let departure_time = args.departure_time;

    // Define allowed transit modes
    let transit_modes = vec![Mode::Rail, Mode::Tram];

    // Process each batch of records
    for batch in batches {
        let num_rows = batch.num_rows();

        // Extract columns - now all casts are already done by DataFusion
        let id_origin = batch
            .column_by_name("id_origin")
            .expect("id_origin")
            .as_string::<i32>();
        let id_dest = batch
            .column_by_name("id_dest")
            .expect("id_dest")
            .as_string::<i32>();
        let lat_origin = batch
            .column_by_name("lat_origin")
            .expect("lat_origin")
            .as_primitive::<Float64Type>();
        let lon_origin = batch
            .column_by_name("lon_origin")
            .expect("lon_origin")
            .as_primitive::<Float64Type>();
        let lat_dest = batch
            .column_by_name("lat_dest")
            .expect("lat_dest")
            .as_primitive::<Float64Type>();
        let lon_dest = batch
            .column_by_name("lon_dest")
            .expect("lon_dest")
            .as_primitive::<Float64Type>();

        // Process each row
        for i in 0..num_rows {
            let id_origin_val = id_origin.value(i);
            let id_dest_val = id_dest.value(i);
            let from_place = format!("{},{}", lat_origin.value(i), lon_origin.value(i));
            let to_place = format!("{},{}", lat_dest.value(i), lon_dest.value(i));

            println!(
                "Processing: {} -> {} ({} -> {})",
                id_origin_val, id_dest_val, from_place, to_place
            );

            match client
                .plan()
                .from_place(&from_place)
                .to_place(&to_place)
                .time(departure_time)
                .transit_modes(transit_modes.clone())
                .detailed_transfers(false)
                .send()
                .await
            {
                Ok(res) => println!("  Result: {res:?}"),
                Err(e) => eprintln!("  Error: {e}"),
            }
        }
    }

    Ok(())
}
