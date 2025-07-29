use std::sync::Arc;

use env_logger;
use log::info;
use zarrs::{
    group::{Group, GroupMetadata},
    storage::ReadableStorage,
};
use zarrs_http::HTTPStore;
use zarrs_storage::storage_adapter::performance_metrics::PerformanceMetricsStorageAdapter;

const ENDPOINT_URL: &str = "https://cci-ke-o.s3-ext.jc.rl.ac.uk/";
const BUCKET: &str = "esacci";
const ZARR_FILE: &str = "ESACCI-L3C_SNOW-SWE-1979-2020-fv2.0.zarr";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    info!("Hello, world!");

    let url = format!("{}/{}/{}", ENDPOINT_URL, BUCKET, ZARR_FILE);
    let http_store: ReadableStorage = Arc::new(HTTPStore::new(&url)?);
    let store = Arc::new(PerformanceMetricsStorageAdapter::new(http_store));

    let group = Group::open(store.clone(), "/")?;
    let metadata = group.metadata();
    match metadata {
        GroupMetadata::V3(m) => println!("{}", m.to_string_pretty()),
        GroupMetadata::V2(m) => println!("{}", m.to_string_pretty()),
    }

    println!("Bytes read: {}", store.bytes_read());
    Ok(())
}
