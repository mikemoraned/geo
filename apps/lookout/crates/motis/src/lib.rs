#[allow(clippy::doc_lazy_continuation)]
pub mod api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}
pub mod bronze;
pub mod client;
pub mod ingest;
pub mod poll;
pub mod source;
pub mod window;
