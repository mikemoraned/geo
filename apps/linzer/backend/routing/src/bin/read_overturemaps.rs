use arrow::array::AsArray;
use clap::Parser;
use datafusion::datasource::file_format::parquet::ParquetFormat;
use datafusion::datasource::listing::{ListingOptions, ListingTable, ListingTableConfig, ListingTableUrl};
use datafusion::prelude::*;
use futures::StreamExt;
use std::sync::Arc;
use geozero::ToGeo;

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
    println!("path: {}", &table_path);

    // Create default parquet options
    let file_format = ParquetFormat::new();
    let listing_options = ListingOptions::new(Arc::new(file_format))
        .with_file_extension(".parquet");
    println!("listing options: {:?}", &listing_options);

    // Resolve the schema
    let resolved_schema = listing_options
        .infer_schema(&session_state, &table_path)
        .await?;
    println!("schema: {:?}", &resolved_schema);

    let config = ListingTableConfig::new(table_path)
        .with_listing_options(listing_options)
        .with_schema(resolved_schema);
    // println!("config: {:?}", &config);

    // Create a new TableProvider
    let provider = Arc::new(ListingTable::try_new(config)?);

    // This provider can now be read as a dataframe:
    // let df = ctx.read_table(provider.clone());

    ctx.register_table("division_area", provider)?;

    let df = ctx.sql("SELECT id,geometry FROM division_area LIMIT 1").await?;
    df.clone().show().await?;

    let mut stream = df.execute_stream().await?;
    while let Some(b) = stream.next().await.transpose()? {
        let id_col  = b.column(0).as_string_view();
        let geometry_col = b.column(1).as_binary_view();
        for (id, geometry) in id_col.iter().zip(geometry_col.iter()) {
            if let (Some(id), Some(geometry)) = (id, geometry) {
                println!("id: {:?}", id);
                let wkb = geozero::wkb::Wkb(geometry.to_vec());
                let geometry = wkb.to_geo()?;
                println!("geometry: {:?}", &geometry);
            }
        }
    }

    Ok(())
}
