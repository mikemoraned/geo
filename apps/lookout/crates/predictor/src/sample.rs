//! One GPS fix, in the terms a predictor works in.
//!
//! Receivers disagree about what a fix comes with. An NMEA receiver reports satellites and
//! HDOP and says nothing about accuracy. A phone reports accuracy in metres and says nothing
//! about satellites. Both leave fields out until they have a fix worth reporting. So a sample
//! carries a position and an instant, and everything else is an [`Option`].
//!
//! Units are metric and named in the field: metres, metres per second, and degrees. Whatever
//! builds a sample converts on the way in — NMEA reports speed in knots — so nothing
//! downstream has to ask which unit it is holding.

use chrono::{DateTime, Utc};
use geo_types::Point;
use serde::{Deserialize, Serialize};

use crate::measure::Measure;

/// A coordinate off the globe.
///
/// It holds `f64` whatever the sample measures in, because a coordinate is checked before it
/// is converted — the value worth reporting is the one that was wrong.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum CoordinateError {
    #[error("latitude {0} outside -90..=90")]
    Latitude(f64),
    #[error("longitude {0} outside -180..=180")]
    Longitude(f64),
}

/// A position from degrees of latitude and longitude, in the axis order georust uses: `x` is
/// the longitude and `y` the latitude.
///
/// This is where a coordinate is checked, and where it takes on the measure. Sentences arrive
/// corrupt and columns arrive unchecked, so every path into a [`Sample`] comes through here.
pub fn position<T: Measure>(
    latitude_degrees: f64,
    longitude_degrees: f64,
) -> Result<Point<T>, CoordinateError> {
    if !(-90.0..=90.0).contains(&latitude_degrees) {
        return Err(CoordinateError::Latitude(latitude_degrees));
    }
    if !(-180.0..=180.0).contains(&longitude_degrees) {
        return Err(CoordinateError::Longitude(longitude_degrees));
    }
    Ok(Point::new(
        degrees(longitude_degrees),
        degrees(latitude_degrees),
    ))
}

/// A checked degree value in the measure.
///
/// Infallible by construction: `from_f64` only declines a value the target cannot represent,
/// and every float can hold a number between -180 and 180.
fn degrees<T: Measure>(value: f64) -> T {
    T::from_f64(value).expect("a degree fits in any float")
}

/// A reading in the measure, dropping one the measure cannot hold.
fn measured<T: Measure>(value: Option<f64>) -> Option<T> {
    value.and_then(T::from_f64)
}

/// One GPS fix: where, when, and whatever else the receiver knew.
///
/// Build one with [`Sample::at`] and add what the source has, a field at a time. Each `with_`
/// method names the one field it sets, so no call site depends on the order of two arguments
/// of the same type. They take `f64` and convert, since that is what every source hands over
/// whatever the sample holds it in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sample<T: Measure> {
    /// When the receiver fixed this position, not when anything downstream saw it.
    pub t: DateTime<Utc>,
    /// Degrees, longitude in `x` and latitude in `y`. Read it through [`Sample::latitude`]
    /// and [`Sample::longitude`] where which is which matters.
    pub position: Point<T>,
    pub altitude_metres: Option<T>,
    pub speed_mps: Option<T>,
    /// Course over ground, degrees clockwise from true north. A stationary receiver reports
    /// no course, since there is none to report.
    pub heading_degrees: Option<T>,
    /// How far out the position is, as the source judged it.
    pub accuracy_metres: Option<T>,
    pub satellites: Option<u32>,
    /// Horizontal dilution of precision: how much the satellite geometry multiplies the
    /// error. Lower is better, and above about 5 a position wanders metres a second.
    pub hdop: Option<T>,
}

impl<T: Measure> Sample<T> {
    /// A fix carrying only what every fix has: where it was and when.
    pub fn new(t: DateTime<Utc>, position: Point<T>) -> Self {
        Self {
            t,
            position,
            altitude_metres: None,
            speed_mps: None,
            heading_degrees: None,
            accuracy_metres: None,
            satellites: None,
            hdop: None,
        }
    }

    /// The same from degrees, for a caller holding a store's columns or a parsed sentence
    /// rather than a checked position.
    pub fn at(
        t: DateTime<Utc>,
        latitude_degrees: f64,
        longitude_degrees: f64,
    ) -> Result<Self, CoordinateError> {
        Ok(Self::new(t, position(latitude_degrees, longitude_degrees)?))
    }

    pub fn latitude(&self) -> T {
        self.position.y()
    }

    pub fn longitude(&self) -> T {
        self.position.x()
    }

    pub fn with_altitude_metres(self, altitude_metres: Option<f64>) -> Self {
        Self {
            altitude_metres: measured(altitude_metres),
            ..self
        }
    }

    pub fn with_speed_mps(self, speed_mps: Option<f64>) -> Self {
        Self {
            speed_mps: measured(speed_mps),
            ..self
        }
    }

    pub fn with_heading_degrees(self, heading_degrees: Option<f64>) -> Self {
        Self {
            heading_degrees: measured(heading_degrees),
            ..self
        }
    }

    pub fn with_accuracy_metres(self, accuracy_metres: Option<f64>) -> Self {
        Self {
            accuracy_metres: measured(accuracy_metres),
            ..self
        }
    }

    pub fn with_satellites(self, satellites: Option<u32>) -> Self {
        Self { satellites, ..self }
    }

    pub fn with_hdop(self, hdop: Option<f64>) -> Self {
        Self {
            hdop: measured(hdop),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant() -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
    }

    #[test]
    fn coordinates_outside_the_valid_range_are_rejected() {
        assert_eq!(
            position::<f64>(91.0, 8.5),
            Err(CoordinateError::Latitude(91.0))
        );
        assert_eq!(
            position::<f64>(50.5, -181.0),
            Err(CoordinateError::Longitude(-181.0))
        );
        assert!(position::<f64>(50.5, 8.5).is_ok());
    }

    /// The axes are the same type, so swapping them is silent.
    #[test]
    fn a_position_holds_its_axes_the_way_round_georust_does() {
        let sample = Sample::<f64>::at(instant(), 50.5, 8.5).expect("on the globe");

        assert_eq!(sample.position, Point::new(8.5, 50.5));
        assert_eq!(sample.latitude(), 50.5);
        assert_eq!(sample.longitude(), 8.5);
    }

    /// The same fix, held in the measure the device uses. `f32` resolves a degree to about a
    /// tenth of a metre here, so the position survives the conversion at the precision the
    /// scan needs.
    #[test]
    fn a_sample_can_be_held_in_the_measure_the_device_uses() {
        let sample = Sample::<f32>::at(instant(), 50.5, 8.5).expect("on the globe");

        assert!((sample.latitude() - 50.5).abs() < 1e-5);
        assert!((sample.longitude() - 8.5).abs() < 1e-5);
    }

    /// A coordinate is checked before it is converted, so the error reports the value that
    /// was actually wrong rather than what it became.
    #[test]
    fn a_sample_off_the_globe_is_refused_in_either_measure() {
        assert_eq!(
            Sample::<f32>::at(instant(), 91.0, 8.5).unwrap_err(),
            CoordinateError::Latitude(91.0)
        );
        assert_eq!(
            Sample::<f64>::at(instant(), 91.0, 8.5).unwrap_err(),
            CoordinateError::Latitude(91.0)
        );
    }

    #[test]
    fn a_sample_knows_only_where_and_when_until_it_is_told_more() {
        let sample = Sample::<f64>::at(instant(), 50.5, 8.5).expect("on the globe");

        assert_eq!(sample.t, instant());
        assert_eq!(sample.speed_mps, None);
        assert_eq!(sample.hdop, None);
    }

    /// The path the simulation takes: `session_sample`'s columns, with no receiver and no
    /// sentence anywhere in it. The columns it has no counterpart for stay absent rather
    /// than being invented — the phone counts no satellites.
    #[test]
    fn a_sample_can_be_built_from_the_columns_silver_carries() {
        let sample = Sample::<f64>::at(instant(), 50.5, 8.5)
            .expect("on the globe")
            .with_altitude_metres(Some(262.46))
            .with_speed_mps(Some(2.1))
            .with_heading_degrees(Some(79.9))
            .with_accuracy_metres(Some(4.8));

        assert_eq!(sample.altitude_metres, Some(262.46));
        assert_eq!(sample.speed_mps, Some(2.1));
        assert_eq!(sample.heading_degrees, Some(79.9));
        assert_eq!(sample.accuracy_metres, Some(4.8));
        assert_eq!(sample.satellites, None);
        assert_eq!(sample.hdop, None);
    }

    #[test]
    fn a_sample_survives_a_round_trip() {
        let sample = Sample::<f64>::at(instant(), 50.5, 8.5)
            .expect("on the globe")
            .with_satellites(Some(6))
            .with_hdop(Some(4.4));

        let json = serde_json::to_string(&sample).expect("serialise");

        assert_eq!(
            serde_json::from_str::<Sample<f64>>(&json).expect("deserialise"),
            sample
        );
    }
}
