//! The Crux core the device shell drives.
//!
//! What the device does, minus the hardware. A shell reads the receiver, the battery pin and
//! the clock, and hands each over as an [`Event`]; everything deciding what any of it *means*
//! is here, where a laptop can test it. The GPS needs sky view and a cold start of about 23
//! seconds, and a whole battery discharge takes an hour and a half, so neither is something
//! to iterate against on the board.
//!
//! The prediction itself is [`predictor`]'s, and so is everything a shell measuring in another
//! float would need. This crate holds what only the board has: the crossings carried in its
//! flash, the judgement of its battery, and its panel.

pub mod app;
pub mod battery;
pub mod carried;
pub mod panel;
pub mod pointset;

pub use app::{Effect, Event, Lookout, Model};
pub use panel::{NEAREST_ON_SCREEN, ViewModel};

/// The float everything here measures in, and which satisfies [`predictor::Measure`].
///
/// `f32`, because the ESP32's FPU is single precision: `f64` there is emulated in software,
/// and a scan of thousands of crossings against every fix cannot afford that. It is the
/// measure rather than a unit — a position held in it is degrees, and a distance metres.
pub(crate) type Float = f32;
