//! The Crux core every shell drives: what is where, and what is about to be crossed.
//!
//! A shell reads a receiver, a browser's geolocation, a battery pin or a clock. Deciding what
//! any of it means happens here, where a laptop can test it. The same test on the board costs
//! a cold start under open sky, or an hour and a half of discharge.
//!
//! The prediction itself is [`predictor`]'s. This crate holds the state a shell drives it
//! with — the parser, the predictor and the battery — and one rule per event about whether it
//! moved anything worth drawing.
//!
//! What it does not hold is a view. A 135-pixel panel and a browser canvas want different
//! things from the same state, so a shell brings a [`Shell`]: where its crossings come from,
//! and how to project what it shows. One core, a view each, and no second copy of the
//! prediction state to drift from this one.

pub mod app;
pub mod battery;
pub mod pointset;

pub use app::{Effect, Event, Lookout, Model, Shell};

/// The float everything here measures in, and one [`domain::Measure`] admits.
///
/// `f32`, because the ESP32's FPU is single precision: `f64` there runs in software, and a
/// scan of thousands of crossings a second cannot afford it. The browser would not mind
/// `f64`, but one core means one float, and the board is the one with no choice. A measure,
/// not a unit — a position held in it is degrees, a distance metres.
pub type Float = f32;
