//! Motis capture + ingest: poll the local Motis server for train trips near recently
//! logged GPS positions, write the raw segments to the bronze capture log, then dedup
//! and decode them into the silver `train_segment` dataset.
//!
//!   - [`window`] — a rolling set of recent GPS positions and the buffered bbox they span.
//!   - [`client`] — a thin wrapper over the Motis `map/trips` endpoint.
//!   - [`bronze`] — the immutable capture log of returned segments, one file per poll.
//!   - [`poll`] — the core of one poll tick, wrapped by the `motis_poll` binary.
//!   - [`ingest`] — dedup + decode the capture log into the silver `train_segment` dataset.

pub mod bronze;
pub mod client;
pub mod ingest;
pub mod poll;
pub mod window;
