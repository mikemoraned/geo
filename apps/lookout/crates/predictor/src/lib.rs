//! The crow-flies predictor, and the sample it works from.
//!
//! Everything here runs on the device and in the runner replaying a recorded session alike,
//! so it reads no store and touches no hardware. What reaches it is a [`Sample`]: one GPS fix
//! in metric units, however it was produced. The device builds samples from its receiver's
//! sentences, through [`Parser`]. The runner builds them from the columns silver holds.

pub mod crossing;
pub mod crow_flies;
/// Sentences in the shape the receiver emits them, shared by the tests either side of the
/// parser. Off unless the `fixtures` feature is on, so none of it reaches a device binary.
#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;
pub mod measure;
pub mod parser;
pub mod predict;
pub mod sample;

pub use crossing::{Crossing, CrossingId, Crossings};
pub use crow_flies::{CrowFlies, DEFAULT_RADIUS_METRES};
pub use measure::Measure;
pub use parser::Parser;
pub use predict::{Event, ObserveError, Predict, Prediction, Trend, Trending};
pub use sample::{CoordinateError, Sample, position};
