//! Transport enrichment: reads a lookout SQLite archive and derives the spatial
//! extent of its GPS data, to fetch Overture transport data intersecting that extent.
//!
//!   - [`extract`] — take a point-in-time Overture extract into bronze.
//!   - [`groups`] — group fixes by `(device_id, UTC day)` and reduce each to a bbox.
//!   - [`archive`] — read the `gps` table those groups are built from.
//!   - [`overture`] — query Overture transport GeoParquet via SedonaDB.
//!   - [`store`] — persist the fetched segments/connectors into the `transport` table.

pub mod archive;
pub mod extract;
pub mod groups;
pub mod overture;
pub mod store;
