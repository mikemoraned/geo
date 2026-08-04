//! The crow-flies predictor, and the sample it works from.
//!
//! Everything here runs on the device and in the simulation alike, so it reads no store and
//! touches no hardware. What reaches it is a [`Sample`]: one GPS fix in metric units, however
//! it was produced. The device makes samples from the sentences its receiver emits, through
//! [`Parser`]; the simulation makes them from the columns silver already holds.

pub mod parser;
pub mod sample;

pub use parser::Parser;
pub use sample::{CoordinateError, Sample, position};
