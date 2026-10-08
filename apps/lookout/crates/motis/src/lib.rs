#[allow(clippy::doc_lazy_continuation)]
pub mod api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}
pub mod bronze;
pub mod client;
pub mod details;
pub mod ingest;
pub mod poll;
pub mod segment;
pub mod source;
pub mod window;
