//! The float a sample, a crossing, and the distance between them are held in.

use geo::CoordFloat;
use num_traits::FromPrimitive;

/// What everything in this crate measures in.
///
/// It is a parameter rather than a fixed type because this crate runs on two platforms that
/// want different answers. The ESP32's FPU is single precision, so `f64` there is emulated in
/// software, and a scan of thousands of crossings against every fix cannot afford that. `f32`
/// resolves about 0.42m at these latitudes, finer than the fix being measured. Off the device
/// `f64` costs nothing and is what the store already holds.
///
/// Degrees enter as `f64` whatever the measure, because that is what every source hands over:
/// NMEA parses to `f64`, and silver stores `f64`. They convert once, where they are checked.
///
/// The bound is what georust's haversine needs, named here so no signature has to repeat it.
pub trait Measure: CoordFloat + FromPrimitive {}

impl<T: CoordFloat + FromPrimitive> Measure for T {}
