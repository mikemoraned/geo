//! The Crux core every shell drives: the device, the rerun runner, and whatever comes after.
//!
//! What the device does, minus the hardware. A shell reads the receiver, the battery pin and
//! the clock, and hands each over as an [`Event`]; everything deciding what any of it *means*
//! is here, where a laptop can test it. The GPS needs sky view and a cold start of about 23
//! seconds, and a whole battery discharge takes an hour and a half, so neither is something
//! to iterate against on the board.
//!
//! The prediction itself is [`predictor`]'s. This crate holds the crossings it scans, the
//! battery judgement, and the panel.

pub mod app;
pub mod battery;
pub mod carried;
pub mod panel;
pub mod pointset;

pub use app::{Effect, Event, Lookout, Model};
pub use panel::{NEAREST_ON_SCREEN, ViewModel};

/// How far ahead the predictor reports. Wide enough that a train at speed has a minute or two
/// of warning, narrow enough to mean something at walking pace.
pub const WITHIN_METRES: f64 = 5_000.0;

/// The float everything here measures in, and which satisfies [`predictor::Measure`].
///
/// `f32`, because the ESP32's FPU is single precision: `f64` there is emulated in software,
/// and a scan of thousands of crossings against every fix cannot afford that. It is the
/// measure rather than a unit — a position held in it is degrees, and a distance metres.
pub(crate) type Float = f32;
