use clap::Parser;
use duckdb::{Connection, Result};
use std::path::PathBuf;

/// Load some data from overturemaps
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// input partitioned root directory
    #[arg(long)]
    overturemaps: String,

    /// output duckdb file
    #[arg(long)]
    duckdb: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("{:?}", args);

    let db = Connection::open(&args.duckdb)?;

    db.execute_batch("INSTALL spatial; LOAD spatial;")?;

    db.execute_batch(&format!("
        CREATE OR REPLACE TABLE division_area
        AS
        SELECT *
        FROM
        read_parquet('{}/theme=divisions/type=division_area/*', hive_partitioning=1)
        ", args.overturemaps)
    )?;

    Ok(())
}
