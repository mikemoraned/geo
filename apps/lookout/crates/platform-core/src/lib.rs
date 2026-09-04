//! The Crux core the device shell drives.
//!
//! What the device does, minus the hardware. A shell reads the receiver, the battery pin, and
//! the clock. Deciding what any of it means happens here, where a laptop can test it. The same
//! test on the board costs a cold start under open sky, or an hour and a half of discharge.
//!
//! The prediction itself is [`predictor`]'s, along with anything a shell measuring in another
//! float needs. This crate holds what only the board has: the crossings in its flash, the
//! judgement of its battery, and its panel.

pub mod app;
pub mod battery;
pub mod carried;
pub mod panel;
pub mod pointset;

pub use app::{Effect, Event, Lookout, Model};
pub use panel::{NEAREST_ON_SCREEN, ViewModel};

/// The float everything here measures in, and one [`predictor::Measure`] admits.
///
/// `f32`, because the ESP32's FPU is single precision: `f64` there runs in software, and a
/// scan of thousands of crossings a second cannot afford it. A measure, not a unit — a
/// position held in it is degrees, a distance metres.
pub(crate) type Float = f32;
