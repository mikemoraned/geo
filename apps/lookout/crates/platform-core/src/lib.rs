//! The Crux core every shell drives: what is where, and what is about to be crossed.
//!
//! A shell reads a receiver, a browser's geolocation, a battery pin or a clock. Deciding what
//! any of it means happens here, where a laptop can test it. The same test on the board costs
//! a cold start under open sky, or an hour and a half of discharge.
//!
//! The prediction itself is [`predictor`]'s. This crate holds the state a shell drives it
//! with: the parser, the predictor and the battery. It adds one rule per event about whether
//! that event moved anything worth drawing.
//!
//! What it does not hold is a view. A 135-pixel panel and a browser canvas want different
//! things from the same state. So a shell brings a [`Shell`]: how it holds the set it scans,
//! and how to project what it shows.
//!
//! It also brings what its platform can *do*, which is the split between [`standalone`] and
//! [`connected`]. The core asks a platform carrying its crossings only to draw, and asks one
//! that can call out for a set as well. Each kind has an app and an effect enum of its own,
//! because crux builds one effect enum per app. The state stays one: [`Model`] and its
//! transitions are shared, so there is no second copy to drift.

pub mod app;
pub mod battery;
pub mod connected;
pub mod pointset;
pub mod standalone;

#[cfg(test)]
mod fixtures;

pub use app::{Event, Model, Shell};
pub use connected::Connected;
pub use standalone::Standalone;

/// The float everything here measures in, and one [`domain::Precision`] admits.
///
/// `f32`, because the ESP32's FPU is single precision. `f64` there runs in software, and a
/// scan of thousands of crossings a second cannot afford it. The browser can hold `f64`, but
/// one core means one float, and the board is the one with no choice. A precision, not a
/// unit — a position held in it is degrees, a distance metres.
pub type Float = f32;
