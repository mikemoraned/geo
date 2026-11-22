#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test reports failure by panicking"
)]

mod auth;
mod caching;
mod compression;
mod gate;
mod ranges;
mod status;
mod support;
mod version;
mod ws_roundtrip;
