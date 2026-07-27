//! Spike: does an Overture row survive `SELECT *` through SedonaDB into our GeoParquet
//! writer with its shape intact?
//!
//! Bronze holds upstream extracts in the upstream's own shape, and the water-crossings
//! notebooks read `bbox.xmin`, `rail_flags` and `ST_Intersects(geometry, …)` straight off
//! Overture's columns. So the nested struct/list columns have to land unaltered and the
//! geometry has to stay a GeoParquet geometry rather than a bare WKB blob. This asks
//! whether that holds before an extractor is built on the assumption that it does.

use medallion::{DatasetSpec, Layer, Root};
use transport::overture::{Overture, OvertureType, Release, DEFAULT_RELEASE};

const SPIKE: DatasetSpec = DatasetSpec::partitioned(Layer::Bronze, "spike", "extract_id");

/// Reads the mirror when `OVERTURE_MIRROR` names one, else the public bucket.
fn release() -> Release {
    match std::env::var("OVERTURE_MIRROR") {
        Ok(path) => Release::mirrored(DEFAULT_RELEASE, path),
        Err(_) => Release::published(DEFAULT_RELEASE),
    }
}

#[tokio::test]
async fn overture_rows_round_trip_into_geoparquet_end_to_end() {
    let overture = Overture::open(release());
    overture
        .register(OvertureType::SEGMENT, "segments")
        .await
        .unwrap();
    // Any rows will do: this asks what the columns are, not what is in them. A predicate
    // would make the scan sweep row-group statistics across the whole global partition
    // hunting for matches, where an unfiltered LIMIT stops at the first row group.
    let batches = overture
        .sql("SELECT *, 'x' AS extract_id FROM segments LIMIT 20")
        .await
        .unwrap();
    let first = batches.first().expect("some rows");
    println!("ROWS {} BATCHES {}", first.num_rows(), batches.len());
    for f in first.schema().fields() {
        println!(
            "FIELD {} :: {:?} meta={:?}",
            f.name(),
            f.data_type(),
            f.metadata()
        );
    }

    let tmp = tempfile::tempdir().unwrap();
    let path = Root::new(tmp.path())
        .dataset(SPIKE)
        .for_id("x")
        .unwrap()
        .rebuild_geo(&batches)
        .await;
    println!("WROTE {path:?}");
}
