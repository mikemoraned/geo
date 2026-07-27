//! `enrich`: derive per-`(device, UTC day)` bounding boxes from a lookout SQLite
//! archive's `gps` table, then fetch the Overture rail `segment`s intersecting them.
//! Later slice tasks fetch the referenced connectors and persist the results to a
//! `transport` table.

use std::path::PathBuf;

use clap::Parser;
use transport::{
    archive::Archive,
    overture::{Overture, Release, DEFAULT_RELEASE},
    store::Store,
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
            min_lat = g.bbox.min().y,
            max_lat = g.bbox.max().y,
            min_lon = g.bbox.min().x,
            max_lon = g.bbox.max().x,
            "bbox",
        );
    }

    // Fetch the Overture rail segments intersecting any of the bounding boxes.
    if groups.is_empty() {
        tracing::warn!("no bounding boxes; skipping Overture fetch");
        return;
    }
    let bboxes: Vec<_> = groups.iter().map(|g| g.bbox).collect();
    let overture = Overture::open(Release::published(&args.release));
    overture
        .register_segments()
        .await
        .expect("register Overture segments from S3");
    overture
        .register_connectors()
        .await
        .expect("register Overture connectors from S3");

    let segments = overture
        .rail_segments(&bboxes)
        .await
        .expect("query Overture rail segments");
    let connectors = overture
        .rail_connectors(&bboxes)
        .await
        .expect("query Overture rail connectors");

    let segment_rows: usize = segments.iter().map(|b| b.num_rows()).sum();
    let connector_rows: usize = connectors.iter().map(|b| b.num_rows()).sum();
    tracing::info!(
        release = %args.release,
        bboxes = bboxes.len(),
        segments = segment_rows,
        connectors = connector_rows,
        "fetched Overture rail transport",
    );

    // Persist into the `transport` table of the same archive (idempotent on GERS id).
    let store = Store::open(&args.db).expect("open transport store");
    let stored_segments = store.insert_segments(&segments).expect("persist segments");
    let stored_connectors = store
        .insert_connectors(&connectors)
        .expect("persist connectors");
    tracing::info!(
        db = %args.db.display(),
        stored_segments,
        stored_connectors,
        "persisted transport table",
    );
}
