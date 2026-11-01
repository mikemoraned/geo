#[allow(clippy::doc_lazy_continuation)]
pub mod api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}
pub mod capture;
pub mod client;
pub mod details;
pub mod ingest;
pub mod landing;
pub mod landing_to_bronze_promotion;
pub mod legacy;
pub mod near_gps;
mod placement;
pub mod polling;
pub mod sampling;
pub mod segment;
pub mod source;
pub mod tally;
mod testing;
pub mod train_sessions;
pub mod window;
pub mod zoom;
