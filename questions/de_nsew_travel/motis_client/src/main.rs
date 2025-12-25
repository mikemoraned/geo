use arrow::array::{AsArray, BooleanBuilder, Float64Builder, StringBuilder, UInt32Builder};
use arrow::datatypes::{DataType, Field, Float64Type, Schema};
use arrow::record_batch::RecordBatch;
use chrono::{DateTime, Utc};
use clap::Parser;
use datafusion::prelude::*;
use motis_openapi_progenitor::types::Mode;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the input parquet file containing origin and destination data
    #[arg(short, long)]
    input: String,

    /// Departure time as ISO 8601 string (e.g., "2026-01-10T09:00:00Z")
    #[arg(short = 't', long, value_parser = parse_datetime)]
    departure_time: DateTime<Utc>,

    /// Output parquet file path
    #[arg(short, long)]
    output: String,
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
        .read_parquet(&args.input, ParquetReadOptions::default())
        .await?
        .with_column("id_origin", cast(col("id_origin"), DataType::Utf8))?
        .with_column("id_dest", cast(col("id_dest"), DataType::Utf8))?;

    // Collect all data into memory
    let batches = df.collect().await?;

    // Create MOTIS client
    let client = motis_openapi_progenitor::Client::new("http://localhost:8080");
    let departure_time = args.departure_time;

    let allowed_transit_modes = vec![Mode::Rail, Mode::Tram];
    let max_travel_time_in_minutes = 24 * 60;

    // Build result arrays
    let mut id_origin_builder = StringBuilder::new();
    let mut id_dest_builder = StringBuilder::new();
    let mut lat_origin_builder = Float64Builder::new();
    let mut lon_origin_builder = Float64Builder::new();
    let mut lat_dest_builder = Float64Builder::new();
    let mut lon_dest_builder = Float64Builder::new();
    let mut total_time_builder = UInt32Builder::new();
    let mut success_builder = BooleanBuilder::new();

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
            let lat_origin_val = lat_origin.value(i);
            let lon_origin_val = lon_origin.value(i);
            let lat_dest_val = lat_dest.value(i);
            let lon_dest_val = lon_dest.value(i);
            let from_place = format!("{},{}", lat_origin_val, lon_origin_val);
            let to_place = format!("{},{}", lat_dest_val, lon_dest_val);

            println!(
                "Processing: {} -> {} ({} -> {})",
                id_origin_val, id_dest_val, from_place, to_place
            );

            let result = client
                .plan()
                .from_place(&from_place)
                .to_place(&to_place)
                .arrive_by(true)
                .time(departure_time)
                .transit_modes(allowed_transit_modes.clone())
                .max_travel_time(max_travel_time_in_minutes)
                .detailed_transfers(true)
                .send()
                .await;

            // Append input columns
            id_origin_builder.append_value(id_origin_val);
            id_dest_builder.append_value(id_dest_val);
            lat_origin_builder.append_value(lat_origin_val);
            lon_origin_builder.append_value(lon_origin_val);
            lat_dest_builder.append_value(lat_dest_val);
            lon_dest_builder.append_value(lon_dest_val);

            // Append result columns
            match result {
                Ok(res) => {
                    // Get the shortest itinerary duration
                    let mut shortest_itineraries = res.itineraries.clone();
                    shortest_itineraries.sort_by(|a, b| a.duration.cmp(&b.duration));
                    if let Some(shortest_itinerary) = shortest_itineraries.first() {
                        let duration = shortest_itinerary.duration as u32;
                        total_time_builder.append_value(duration);
                        success_builder.append_value(true);
                        println!("  Success: duration = {} seconds", duration);
                    } else {
                        total_time_builder.append_null();
                        success_builder.append_value(false);
                    }
                }
                Err(e) => {
                    total_time_builder.append_null();
                    success_builder.append_value(false);
                    eprintln!("  Error: {e}");
                }
            }
        }
    }

    // Create output schema
    let schema = Arc::new(Schema::new(vec![
        Field::new("id_origin", DataType::Utf8, false),
        Field::new("id_dest", DataType::Utf8, false),
        Field::new("lat_origin", DataType::Float64, false),
        Field::new("lon_origin", DataType::Float64, false),
        Field::new("lat_dest", DataType::Float64, false),
        Field::new("lon_dest", DataType::Float64, false),
        Field::new("total_time", DataType::UInt32, true),
        Field::new("success", DataType::Boolean, false),
    ]));

    // Build arrays
    let id_origin_array = Arc::new(id_origin_builder.finish());
    let id_dest_array = Arc::new(id_dest_builder.finish());
    let lat_origin_array = Arc::new(lat_origin_builder.finish());
    let lon_origin_array = Arc::new(lon_origin_builder.finish());
    let lat_dest_array = Arc::new(lat_dest_builder.finish());
    let lon_dest_array = Arc::new(lon_dest_builder.finish());
    let total_time_array = Arc::new(total_time_builder.finish());
    let success_array = Arc::new(success_builder.finish());

    // Create record batch
    let output_batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            id_origin_array,
            id_dest_array,
            lat_origin_array,
            lon_origin_array,
            lat_dest_array,
            lon_dest_array,
            total_time_array,
            success_array,
        ],
    )?;

    // Write to parquet
    let output_df = ctx.read_batch(output_batch)?;
    output_df
        .write_parquet(
            &args.output,
            datafusion::dataframe::DataFrameWriteOptions::new(),
            None,
        )
        .await?;

    println!("\nResults written to: {}", args.output);

    Ok(())
}
