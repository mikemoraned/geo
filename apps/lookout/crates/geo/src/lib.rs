//! Geo enrichment: reads a lookout SQLite archive and derives the spatial extent of
//! its GPS data, to later fetch Overture transport data intersecting that extent.
//!
//!   - [`groups`] — group fixes by `(device_id, UTC day)` and reduce each to a bbox.
//!   - [`archive`] — read the `gps` table those groups are built from.

pub mod archive;
pub mod groups;
