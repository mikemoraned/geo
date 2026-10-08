#[allow(clippy::doc_lazy_continuation)]
pub mod api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}
pub mod bronze;
pub mod capture;
pub mod client;
pub mod details;
pub mod ingest;
pub mod near_gps;
pub mod segment;
pub mod source;
pub mod window;
