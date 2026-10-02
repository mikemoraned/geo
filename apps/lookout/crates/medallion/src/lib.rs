//! ```no_run
//! use chrono::Utc;
//! use medallion::{DatasetSpec, Row, layers};
//! use serde::{Deserialize, Serialize};
//!
//! const GPS_READING: DatasetSpec<layers::Bronze> =
//!     DatasetSpec::partitioned("gps_reading", "ingested_date");
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
//!     type Layer = layers::Bronze;
//!     const DATASET: DatasetSpec<Self::Layer> = GPS_READING;
//!     const INSTANTS: &'static [&'static str] = &["t"];
//! }
//!
//! # async fn example(rows: &[GpsReadingRow]) -> Result<(), Box<dyn std::error::Error>> {
//! let now = Utc::now();
//! medallion::Root::new(medallion::Root::default_path()?)
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
pub use datafusion::scalar::ScalarValue;
pub use dataset::{DatasetInfo, DatasetSpec};
pub use derive::{GeoRow, write_geo_rows, write_rows};
pub use geo::{
    GEOMETRY, GeoError, PROJECTED_GEOMETRY, Projector, geo_batch, geometries, projected_wkb_field,
    wkb_column, wkb_field,
};
pub use layer::{Layer, LayerKind, Replaceable, layers};
pub use partition::{Partition, PartitionKey, PartitionValue, PathError};
pub use path::{AppendError, Dataset, ReplaceError, Replaced, Root, Written, gold_version};
pub use query::{Query, QueryError, countries_of, rows_of_every_country, table_references};
pub use rows::{Dated, Geometry, Row, RowError, batch, fields};
pub use table::{SilverTarget, TableError, TableWritten, write_table};
pub use write::WriteError;
