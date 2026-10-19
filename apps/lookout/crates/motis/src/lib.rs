#[allow(clippy::doc_lazy_continuation)]
pub mod api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}
pub mod bronze;
pub mod capture;
pub mod client;
pub mod details;
pub mod ingest;
pub mod legacy;
pub mod near_gps;
pub mod polling;
pub mod sampling;
pub mod segment;
pub mod source;
pub mod span;
pub mod tally;
pub mod window;
pub mod zoom;
