//! The crow-flies predictor, and the sample it works from.
//!
//! Everything here runs on the device and in the simulation alike, so it reads no store and
//! touches no hardware. What reaches it is a [`Sample`]: one GPS fix in metric units, however
//! it was produced. The device makes samples from the sentences its receiver emits, through
//! [`Parser`]; the simulation makes them from the columns silver already holds.

pub mod crossing;
pub mod crow_flies;
pub mod measure;
pub mod parser;
pub mod predict;
pub mod sample;

pub use crossing::{Crossing, CrossingId, Crossings};
pub use crow_flies::CrowFlies;
pub use measure::Measure;
pub use parser::Parser;
pub use predict::{Event, ObserveError, Predict, Prediction, Trend, Trending};
pub use sample::{CoordinateError, Sample, position};
