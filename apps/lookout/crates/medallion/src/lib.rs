//! Paths and writers for the medallion data store described in `docs/medallion.md`.
//!
//! Every CLI that reads or writes the store constructs its paths through here rather than
//! joining strings itself, so the layer names, Hive partition layout and the naming rules
//! partitions must meet live in one place. [`MedallionArgs`] gives each binary the same
//! `--medallion-root` flag and default.
//!
//! ```no_run
//! use chrono::Utc;
//! use medallion::{write_batches, Layer, Root};
//!
//! # async fn example(batches: &[arrow::array::RecordBatch]) -> Result<(), Box<dyn std::error::Error>> {
//! let path = Root::default()
//!     .dataset(Layer::Bronze, "sensor_reading")
//!     .partition("sensor", "gps")?
//!     .date_partition("ingested_date", Utc::now().date_naive())?
//!     .batch_file(Utc::now());
//! write_batches(&path, batches).await?;
//! # Ok(())
//! # }
//! ```

mod args;
mod geo;
mod layer;
mod partition;
mod path;
mod write;

pub use args::MedallionArgs;
pub use geo::{wkb_field, write_geo_batches, GeoError};
pub use layer::Layer;
pub use partition::{Partition, PartitionKey, PartitionValue, PathError};
pub use path::{Dataset, Root};
pub use write::{write_batches, WriteError};
