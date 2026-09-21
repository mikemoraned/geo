//! A fix, and when it was taken.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::gps::Gps;
use crate::position::CoordinateError;
use crate::precision::Precision;

/// One reported fix: what a source knew about where it was, and the instant it knew it for.
///
/// The instant is the reading's, not the moment anything received it. A browser's
/// `watchPosition` fix is seconds old by the time it arrives, and a replayed one is months
/// old, so whoever handles a sample reads the time from the sample rather than from a clock.
///
/// Held in the float the platform works in, since a scan subtracts a fix from a crossing: a
/// source reports degrees in `f64`, and the board measures in `f32`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct Sample<P: Precision> {
    pub t: DateTime<Utc>,
    pub gps: Gps<P>,
}

impl<P: Precision> Sample<P> {
    pub fn new(t: DateTime<Utc>, gps: Gps<P>) -> Self {
        Self { t, gps }
    }

    /// A fix from degrees, latitude first, for a caller holding a store's columns or a parsed
    /// sentence rather than a checked position.
    ///
    /// # Errors
    ///
    /// Returns an error where the coordinates are not on the globe.
    pub fn at(
        t: DateTime<Utc>,
        latitude_degrees: f64,
        longitude_degrees: f64,
    ) -> Result<Self, CoordinateError> {
        Ok(Self::new(t, Gps::at(latitude_degrees, longitude_degrees)?))
    }

    pub fn latitude(&self) -> P {
        self.gps.latitude()
    }

    pub fn longitude(&self) -> P {
        self.gps.longitude()
    }
}

/// What a source knew, set a field at a time. Each names the one field it sets, so no call
/// site depends on the order of two arguments of the same type, and each delegates to the fix
/// itself — a sample adds only the instant.
impl<P: Precision> Sample<P> {
    pub fn with_altitude_metres(self, altitude_metres: Option<f64>) -> Self {
        Self {
            gps: self.gps.with_altitude_metres(altitude_metres),
            ..self
        }
    }

    pub fn with_accuracy_metres(self, accuracy_metres: Option<f64>) -> Self {
        Self {
            gps: self.gps.with_accuracy_metres(accuracy_metres),
            ..self
        }
    }

    pub fn with_speed_mps(self, speed_mps: Option<f64>) -> Self {
        Self {
            gps: self.gps.with_speed_mps(speed_mps),
            ..self
        }
    }

    pub fn with_heading_degrees(self, heading_degrees: Option<f64>) -> Self {
        Self {
            gps: self.gps.with_heading_degrees(heading_degrees),
            ..self
        }
    }

    pub fn with_satellites(self, satellites: Option<u32>) -> Self {
        Self {
            gps: self.gps.with_satellites(satellites),
            ..self
        }
    }

    pub fn with_hdop(self, hdop: Option<f64>) -> Self {
        Self {
            gps: self.gps.with_hdop(hdop),
            ..self
        }
    }

    /// The same sample at another precision. See [`Gps::to_precision`].
    ///
    /// # Errors
    ///
    /// Returns an error where the coordinates are not on the globe.
    pub fn to_precision<Q: Precision>(&self) -> Result<Sample<Q>, CoordinateError> {
        Ok(Sample::new(self.t, self.gps.to_precision()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant() -> DateTime<Utc> {
        DateTime::from_timestamp(1_785_098_609, 0).expect("an instant")
    }

    fn sample() -> Sample<f64> {
        Sample::at(instant(), 51.0403, 13.7322)
            .expect("on the globe")
            .with_speed_mps(Some(27.8))
            .with_accuracy_metres(Some(5.0))
    }

    /// The shape a kiosk replays and a shell dispatches, written once here rather than at each
    /// end: a page sends what it read, and reshapes nothing.
    #[test]
    fn a_sample_is_written_as_an_instant_beside_the_fix() {
        let json = serde_json::to_string(&sample()).expect("write");

        assert_eq!(
            json,
            r#"{"t":"2026-07-26T20:43:29Z","gps":{"lat":51.0403,"lon":13.7322,"alt":null,"acc":5.0,"speed":27.8,"heading":null}}"#
        );
    }

    #[test]
    fn a_sample_knows_only_where_and_when_until_it_is_told_more() {
        let sample = Sample::<f64>::at(instant(), 50.5, 8.5).expect("on the globe");

        assert_eq!(sample.t, instant());
        assert_eq!(sample.gps.speed_mps, None);
        assert_eq!(sample.gps.hdop, None);
        assert_eq!(sample.gps.accuracy_metres, None);
    }

    /// The path the runner takes: `session_sample`'s columns, with no receiver and no
    /// sentence anywhere in it. The columns it has no counterpart for stay absent rather
    /// than being invented — a phone counts no satellites.
    #[test]
    fn a_sample_can_be_built_from_the_columns_silver_carries() {
        let sample = Sample::<f64>::at(instant(), 50.5, 8.5)
            .expect("on the globe")
            .with_altitude_metres(Some(262.46))
            .with_speed_mps(Some(2.1))
            .with_heading_degrees(Some(79.9))
            .with_accuracy_metres(Some(4.8));

        assert_eq!(sample.gps.altitude_metres, Some(262.46));
        assert_eq!(sample.gps.speed_mps, Some(2.1));
        assert_eq!(sample.gps.heading_degrees, Some(79.9));
        assert_eq!(sample.gps.accuracy_metres, Some(4.8));
        assert_eq!(sample.gps.satellites, None);
        assert_eq!(sample.gps.hdop, None);
    }

    /// A coordinate is checked before it is converted, so the error reports the value that
    /// was actually wrong rather than what it became.
    #[test]
    fn a_sample_off_the_globe_is_refused_at_either_precision() {
        assert_eq!(
            Sample::<f32>::at(instant(), 91.0, 8.5).unwrap_err(),
            CoordinateError::Latitude(91.0)
        );
        assert_eq!(
            Sample::<f64>::at(instant(), 91.0, 8.5).unwrap_err(),
            CoordinateError::Latitude(91.0)
        );
    }

    /// How a reported fix reaches the board: everything the source knew, in the float the
    /// scan measures in.
    #[test]
    fn a_sample_carries_what_it_knew_into_another_precision() {
        let held: Sample<f32> = sample()
            .with_satellites(Some(6))
            .with_hdop(Some(4.4))
            .to_precision()
            .expect("on the globe");

        assert!((held.latitude() - 51.0403).abs() < 1e-4);
        assert_eq!(held.gps.speed_mps, Some(27.8));
        assert_eq!(held.gps.accuracy_metres, Some(5.0));
        assert_eq!(held.gps.satellites, Some(6));
    }

    /// A fix read off a wire is read unchecked, so the conversion into the precision a scan
    /// runs in is where an impossible position is stopped.
    #[test]
    fn a_sample_read_off_the_globe_is_refused_on_the_way_into_a_scan() {
        let read: Sample<f64> = serde_json::from_str(
            r#"{"t":"2026-07-26T20:43:29Z","gps":{"lat":91.0,"lon":13.7322,"alt":null,"acc":5.0}}"#,
        )
        .expect("read");

        assert_eq!(
            read.to_precision::<f32>().unwrap_err(),
            CoordinateError::Latitude(91.0)
        );
    }

    #[test]
    fn a_sample_survives_being_sent() {
        let json = serde_json::to_string(&sample()).expect("write");

        assert_eq!(
            serde_json::from_str::<Sample<f64>>(&json).expect("read"),
            sample()
        );
    }
}
