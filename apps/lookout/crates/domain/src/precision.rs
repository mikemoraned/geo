//! How finely a position, a crossing, and the distance between them are held.

use geo_types::CoordFloat;
use num_traits::FromPrimitive;

/// How finely a coordinate is held.
///
/// A parameter rather than a fixed type, because the same crossing is held on two platforms
/// wanting different answers. The ESP32's FPU is single precision, so `f64` there runs in
/// software, and a scan of thousands of crossings against every fix cannot afford it. `f32`
/// resolves about 0.42m at these latitudes, finer than the fix it measures. Off the device
/// `f64` costs nothing and is what the store holds.
///
/// Degrees enter as `f64` whatever the precision, because that is what every source hands over:
/// NMEA parses to `f64`, and silver stores `f64`. They convert once, in
/// [`crate::position::position`], where they are checked.
///
/// The bound is what georust's haversine needs, named here so no signature has to repeat it.
pub trait Precision: CoordFloat + FromPrimitive {}

impl<P: CoordFloat + FromPrimitive> Precision for P {}
