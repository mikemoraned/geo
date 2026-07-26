//! Paths and writers for the medallion data store described in `docs/medallion.md`.
//!
//! Every CLI that reads or writes the store constructs its paths through here rather than
//! joining strings itself, so the layer names, Hive partition layout and the naming rules
//! partitions must meet live in one place. [`MedallionArgs`] gives each binary the same
//! `--medallion-root` flag and default.
//!
//! ```no_run
//! use chrono::Utc;
//! use medallion::{Layer, Root};
//!
//! # async fn example(batches: &[arrow::array::RecordBatch]) -> Result<(), Box<dyn std::error::Error>> {
//! let now = Utc::now();
//! Root::default()
//!     .dataset(Layer::Bronze, "sensor_reading")
//!     .partition("sensor", "gps")?
//!     .date_partition("ingested_date", now.date_naive())?
//!     .append(now, batches)
//!     .await?;
//! # Ok(())
//! # }
//! ```

mod args;
mod geo;
mod layer;
mod partition;
mod path;
mod query;
mod write;

pub use args::MedallionArgs;
pub use geo::{projected_wkb_field, wkb_field, GeoError, Projector};
pub use layer::Layer;
pub use partition::{Partition, PartitionKey, PartitionValue, PathError};
pub use path::{Dataset, Root};
pub use query::{Query, QueryError};
pub use write::WriteError;
