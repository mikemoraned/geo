//! Motis capture + ingest: poll the local Motis server for train trips near recently
//! logged GPS positions, append the raw segments to a `motis` SQLite log, then dedup
//! and decode them into a derived `train_segment` table in the `lookout` db.
//!
//!   - [`window`] — a rolling set of recent GPS positions and the buffered bbox they span.
//!   - [`client`] — a thin wrapper over the Motis `map/trips` endpoint.
//!   - [`store`] — the append-only raw capture log of returned segments.
//!   - [`poll`] — the core of one poll tick, wrapped by the `motis_poll` binary.
//!   - [`ingest`] — dedup + decode the raw log into the derived `train_segment` table.

pub mod client;
pub mod ingest;
pub mod poll;
pub mod store;
pub mod window;
