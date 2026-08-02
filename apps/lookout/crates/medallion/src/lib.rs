//! Paths and writers for the medallion data store described in `docs/medallion.md`.
//!
//! Every CLI that reads or writes the store goes through here rather than joining strings
//! itself, so the layer names, Hive partition layout and the naming rules partitions must
//! meet live in one place. [`MedallionArgs`] gives each binary the same `--medallion-root`
//! flag and default.
//!
//! A dataset is passed around as a [`DatasetSpec`], which carries its layer and partition
//! key, and its columns are the [`Row`] type declared alongside it; the datasets
//! themselves are defined by whoever owns the data, not here.
//!
//! ```no_run
//! use chrono::Utc;
//! use medallion::{DatasetSpec, Layer, Root, Row};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct GpsReadingRow {
//!     device_id: String,
//!     t: i64,
//!     lat: f64,
//!     lon: f64,
//! }
//!
//! impl Row for GpsReadingRow {
//!     const DATASET: DatasetSpec =
//!         DatasetSpec::partitioned(Layer::Bronze, "gps_reading", "ingested_date");
//!     const INSTANTS: &'static [&'static str] = &["t"];
//! }
//!
//! # async fn example(rows: &[GpsReadingRow]) -> Result<(), Box<dyn std::error::Error>> {
//! let now = Utc::now();
//! Root::default()
//!     .rows_of::<GpsReadingRow>()
//!     .on_date(now.date_naive())?
//!     .append_rows(now, rows)
//!     .await?;
//! # Ok(())
//! # }
//! ```

mod args;
mod country;
mod dataset;
mod derive;
mod geo;
mod layer;
mod partition;
mod path;
mod query;
mod rows;
pub mod summary;
mod table;
mod write;

pub use args::MedallionArgs;
pub use country::{COUNTRY, Countries, Country, UnknownCountry};
pub use dataset::{DatasetInfo, DatasetSpec};
pub use derive::{GeoRow, write_geo_rows, write_rows};
pub use geo::{
    GEOMETRY, GeoError, PROJECTED_GEOMETRY, Projector, geo_batch, geometries, projected_wkb_field,
    wkb_column, wkb_field,
};
pub use layer::{Layer, LayerKind, Replaceable, layers};
pub use partition::{Partition, PartitionKey, PartitionValue, PathError};
pub use path::{AppendError, Dataset, ReplaceError, Replaced, Root, Written};
pub use query::{Query, QueryError};
pub use rows::{Dated, Geometry, Row, RowError, batch, fields};
pub use table::{SilverTarget, TableError, TableWritten, write_table};
pub use write::WriteError;
