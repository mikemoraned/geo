//! `enrich`: derive per-`(device, UTC day)` bounding boxes from a lookout SQLite
//! archive's `gps` table. Later slice tasks extend this to fetch Overture transport
//! data intersecting those boxes and persist a `transport` table; for now it reads
//! the archive and reports the boxes it derives.

use std::path::PathBuf;

use clap::Parser;
use transport::archive::Archive;

/// Default SQLite archive path (the recorder's output).
const DEFAULT_DB: &str = "data/lookout.sqlite";

#[derive(Parser)]
#[command(about = "Enrich a lookout archive with Overture transport data")]
struct Args {
    /// Path to the SQLite archive to read the `gps` table from.
    #[arg(long, default_value = DEFAULT_DB)]
    db: PathBuf,
}

fn main() {
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
}
