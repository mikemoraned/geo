use arrow::array::{Float64Array, StringArray};
use arrow::record_batch::RecordBatch;
use chrono::{DateTime, Utc};
use clap::Parser;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::fs::File;

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

    // Open the parquet file
    let file = File::open(&args.parquet_file)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let reader = builder.build()?;

    // Create MOTIS client
    let client = motis_openapi_progenitor::Client::new("http://localhost:8080");
    let departure_time = args.departure_time;

    // Process each batch of records
    for batch_result in reader {
        let batch: RecordBatch = batch_result?;

        // Extract columns
        let id_origin = batch
            .column_by_name("id_origin")
            .and_then(|col| col.as_any().downcast_ref::<StringArray>());
        let id_dest = batch
            .column_by_name("id_dest")
            .and_then(|col| col.as_any().downcast_ref::<StringArray>());
        let lat_origin = batch
            .column_by_name("lat_origin")
            .and_then(|col| col.as_any().downcast_ref::<Float64Array>());
        let lon_origin = batch
            .column_by_name("lon_origin")
            .and_then(|col| col.as_any().downcast_ref::<Float64Array>());
        let lat_dest = batch
            .column_by_name("lat_dest")
            .and_then(|col| col.as_any().downcast_ref::<Float64Array>());
        let lon_dest = batch
            .column_by_name("lon_dest")
            .and_then(|col| col.as_any().downcast_ref::<Float64Array>());

        // Ensure all columns are present
        let (id_origin, id_dest, lat_origin, lon_origin, lat_dest, lon_dest) = match (
            id_origin, id_dest, lat_origin, lon_origin, lat_dest, lon_dest,
        ) {
            (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) => (a, b, c, d, e, f),
            _ => {
                eprintln!("Missing required columns in parquet file");
                continue;
            }
        };

        // Process each row
        for i in 0..batch.num_rows() {
            let from_place = format!("{},{}", lat_origin.value(i), lon_origin.value(i));
            let to_place = format!("{},{}", lat_dest.value(i), lon_dest.value(i));

            println!(
                "Processing: {} -> {} ({} -> {})",
                id_origin.value(i),
                id_dest.value(i),
                from_place,
                to_place
            );

            match client
                .plan()
                .from_place(&from_place)
                .to_place(&to_place)
                .time(departure_time)
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
