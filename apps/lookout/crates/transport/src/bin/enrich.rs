//! `enrich`: derive per-`(device, UTC day)` bounding boxes from a lookout SQLite
//! archive's `gps` table, then fetch the Overture transport data intersecting them.
//! For now it smoke-tests the Overture fetch by reading a few `segment`s intersecting
//! the first bounding box; later slice tasks persist the results to a `transport` table.

use std::path::PathBuf;

use clap::Parser;
use transport::{
    archive::Archive,
    overture::{Overture, DEFAULT_RELEASE},
};

/// Default SQLite archive path (the recorder's output).
const DEFAULT_DB: &str = "data/lookout.sqlite";

#[derive(Parser)]
#[command(about = "Enrich a lookout archive with Overture transport data")]
struct Args {
    /// Path to the SQLite archive: read the `gps` table from it, write `transport` to it.
    #[arg(long, default_value = DEFAULT_DB)]
    db: PathBuf,
    /// Overture release to read from S3 (see docs.overturemaps.org/release).
    #[arg(long, default_value = DEFAULT_RELEASE)]
    release: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "enrich=info".into()),
        )
        .init();

    let args = Args::parse();

    let archive = Archive::open(&args.db).expect("open sqlite archive");
    let groups = archive.groups().expect("derive bounding boxes");

    tracing::info!(
        groups = groups.len(),
        "derived per-(device, day) bounding boxes"
    );
    for g in &groups {
        tracing::info!(
            device_id = %g.key.device_id,
            day = g.key.day,
            min_lat = g.bbox.min_lat,
            max_lat = g.bbox.max_lat,
            min_lon = g.bbox.min_lon,
            max_lon = g.bbox.max_lon,
            "bbox",
        );
    }

    // Smoke test: fetch a few Overture segments intersecting the first bbox.
    let Some(first) = groups.first() else {
        tracing::warn!("no bounding boxes; skipping Overture fetch");
        return;
    };
    let overture = Overture::open(&args.release);
    overture
        .register_segments()
        .await
        .expect("register Overture segments from S3");
    let batches = overture
        .segments_in_bbox(&first.bbox, 5)
        .await
        .expect("query Overture segments");
    let rows: usize = batches.iter().map(|b| b.num_rows()).sum();
    let columns: Vec<String> = batches
        .first()
        .map(|b| {
            b.schema()
                .fields()
                .iter()
                .map(|f| f.name().clone())
                .collect()
        })
        .unwrap_or_default();
    tracing::info!(
        release = %args.release,
        device_id = %first.key.device_id,
        day = first.key.day,
        rows,
        ?columns,
        "smoke: Overture segments intersecting first bbox",
    );
}
