use std::sync::Arc;
use clap::Parser;
use datafusion::datasource::file_format::parquet::ParquetFormat;
use datafusion::datasource::listing::{ListingOptions, ListingTable, ListingTableConfig, ListingTableUrl};
use datafusion::prelude::*;

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

    let ctx = SessionContext::new();
    let session_state = ctx.state();
    let table_path = format!("{}/theme=divisions/type=division_area/", &args.overturemaps);

    // Parse the path
    let table_path = ListingTableUrl::parse(table_path)?;

    // Create default parquet options
    let file_format = ParquetFormat::new();
    let listing_options = ListingOptions::new(Arc::new(file_format))
        .with_file_extension(".parquet");

    // Resolve the schema
    let resolved_schema = listing_options
        .infer_schema(&session_state, &table_path)
        .await?;

    let config = ListingTableConfig::new(table_path)
        .with_listing_options(listing_options)
        .with_schema(resolved_schema);

    // Create a new TableProvider
    let provider = Arc::new(ListingTable::try_new(config)?);

    // This provider can now be read as a dataframe:
    // let df = ctx.read_table(provider.clone());

    ctx.register_table("division_are", provider)?;

    let df = ctx.sql("SELECT COUNT(1) FROM division_are").await?;
    df.show().await?;

    Ok(())
}
