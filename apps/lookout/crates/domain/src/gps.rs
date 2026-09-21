//! A GPS fix, as whatever produced it knew it.

use geo_types::Point;
use serde::{Deserialize, Serialize, Serializer, de::Deserializer};

use crate::position::{CoordinateError, degrees, position};
use crate::precision::Precision;

/// One fix: where, how well, and how fast.
///
/// Every field but the position is optional, because sources disagree about what a fix comes
/// with. A phone reports accuracy in metres and nothing about satellites; a receiver reports
/// satellites and HDOP and nothing about accuracy. Both report no heading when standing
/// still, and an absent one means "not moving" rather than "unknown", so it is kept rather
/// than dropped.
///
/// Held in the float the platform works in: `f64` where a source reported it and a store
/// keeps it, `f32` on a board whose FPU is single precision and which subtracts a fix from
/// thousands of crossings a second.
///
/// **Recordings abbreviate the names and the code does not.** A browser has sent the short
/// names since the first recording and the archive still holds them, so what goes over a wire
/// is written in those names and the fields here are free to be named in full.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gps<P: Precision> {
    /// Degrees, longitude in `x` and latitude in `y`.
    pub position: Point<P>,
    pub altitude_metres: Option<P>,
    /// How far out the fix is, in metres, as the source judged it. Absent from a receiver
    /// that reports [`Gps::hdop`] instead.
    pub accuracy_metres: Option<P>,
    /// Metres per second, Doppler-derived where the source can.
    pub speed_mps: Option<P>,
    /// Course over ground, degrees clockwise from true north. Absent when standing still,
    /// since there is no course to report.
    pub heading_degrees: Option<P>,
    /// How many satellites the fix was taken from, where the source counts them.
    pub satellites: Option<u32>,
    /// Horizontal dilution of precision: how much the satellite geometry multiplies the
    /// error. Lower is better, and above about 5 a position wanders metres a second.
    pub hdop: Option<P>,
}

impl<P: Precision> Gps<P> {
    /// A fix carrying only what every fix has: where it was.
    pub fn new(position: Point<P>) -> Self {
        Self {
            position,
            altitude_metres: None,
            accuracy_metres: None,
            speed_mps: None,
            heading_degrees: None,
            satellites: None,
            hdop: None,
        }
    }

    /// A fix from degrees, latitude first, for a caller holding a store's columns or a parsed
    /// sentence rather than a checked position.
    ///
    /// # Errors
    ///
    /// Returns an error where the coordinates are not on the globe.
    pub fn at(latitude_degrees: f64, longitude_degrees: f64) -> Result<Self, CoordinateError> {
        Ok(Self::new(position(latitude_degrees, longitude_degrees)?))
    }

    pub fn latitude(&self) -> P {
        self.position.y()
    }

    pub fn longitude(&self) -> P {
        self.position.x()
    }

    /// Each `with_` method names the one field it sets, so no call site depends on the order
    /// of two arguments of the same type. They take `f64` and convert, since that is what
    /// every source hands over whatever precision the fix is held at.
    pub fn with_altitude_metres(self, altitude_metres: Option<f64>) -> Self {
        Self {
            altitude_metres: held(altitude_metres),
            ..self
        }
    }

    pub fn with_accuracy_metres(self, accuracy_metres: Option<f64>) -> Self {
        Self {
            accuracy_metres: held(accuracy_metres),
            ..self
        }
    }

    pub fn with_speed_mps(self, speed_mps: Option<f64>) -> Self {
        Self {
            speed_mps: held(speed_mps),
            ..self
        }
    }

    pub fn with_heading_degrees(self, heading_degrees: Option<f64>) -> Self {
        Self {
            heading_degrees: held(heading_degrees),
            ..self
        }
    }

    pub fn with_satellites(self, satellites: Option<u32>) -> Self {
        Self { satellites, ..self }
    }

    pub fn with_hdop(self, hdop: Option<f64>) -> Self {
        Self {
            hdop: held(hdop),
            ..self
        }
    }

    /// The same fix at another precision, which is how a reported `f64` reaches a board that
    /// scans in `f32`.
    ///
    /// # Errors
    ///
    /// Returns an error where the coordinates are not on the globe. A fix read off a wire is
    /// read unchecked — one bad row should not cost an archive — so this is where it is
    /// checked, before an impossible position is subtracted from every crossing.
    pub fn to_precision<Q: Precision>(&self) -> Result<Gps<Q>, CoordinateError> {
        Ok(
            Gps::at(reported(self.latitude()), reported(self.longitude()))?
                .with_altitude_metres(self.altitude_metres.map(reported))
                .with_accuracy_metres(self.accuracy_metres.map(reported))
                .with_speed_mps(self.speed_mps.map(reported))
                .with_heading_degrees(self.heading_degrees.map(reported))
                .with_satellites(self.satellites)
                .with_hdop(self.hdop.map(reported)),
        )
    }
}

/// A reading at the precision asked for, dropping one it cannot hold.
fn held<P: Precision>(value: Option<f64>) -> Option<P> {
    value.and_then(P::from_f64)
}

/// A degree value out of the precision it is held at, for writing one back as reported.
fn reported<P: Precision>(value: P) -> f64 {
    value.to_f64().expect("a degree is a number")
}

/// How a fix is written: the six abbreviated names a browser has sent since the first
/// recording, and which the archive still holds.
///
/// The fields a receiver reports and a browser does not — satellites and HDOP — are not
/// among them. Nothing sends a fix over this wire but a browser, and adding a key to a
/// format an archive is already written in buys nothing.
#[derive(Serialize, Deserialize)]
struct Reported {
    lat: f64,
    lon: f64,
    alt: Option<f64>,
    acc: Option<f64>,
    #[serde(default)]
    speed: Option<f64>,
    #[serde(default)]
    heading: Option<f64>,
}

/// Written as a source reported it, whatever the fix is held in.
impl<P: Precision> Serialize for Gps<P> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Reported {
            lat: reported(self.latitude()),
            lon: reported(self.longitude()),
            alt: self.altitude_metres.map(reported),
            acc: self.accuracy_metres.map(reported),
            speed: self.speed_mps.map(reported),
            heading: self.heading_degrees.map(reported),
        }
        .serialize(serializer)
    }
}

/// Read unchecked, as every recorded fix is: a coordinate off the globe is one row to drop,
/// not a reason to refuse the archive it is in.
impl<'de, P: Precision> Deserialize<'de> for Gps<P> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let read = Reported::deserialize(deserializer)?;
        Ok(Self::new(Point::new(degrees(read.lon), degrees(read.lat)))
            .with_altitude_metres(read.alt)
            .with_accuracy_metres(read.acc)
            .with_speed_mps(read.speed)
            .with_heading_degrees(read.heading))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix() -> Gps<f64> {
        Gps::at(55.95, -3.19)
            .expect("on the globe")
            .with_accuracy_metres(Some(8.5))
            .with_speed_mps(Some(31.4))
    }

    /// The names an archive of recordings is written in. A rename here would strand every fix
    /// already stored.
    #[test]
    fn a_fix_is_written_in_the_names_the_wire_uses() {
        let json = serde_json::to_string(&fix()).expect("serialize");

        assert_eq!(
            json,
            r#"{"lat":55.95,"lon":-3.19,"alt":null,"acc":8.5,"speed":31.4,"heading":null}"#
        );
    }

    /// What a receiver adds is the predictor's to read, and no part of what a browser sends.
    #[test]
    fn what_a_receiver_adds_is_not_written_to_the_wire() {
        let json = serde_json::to_string(&fix().with_satellites(Some(6)).with_hdop(Some(4.4)))
            .expect("serialize");

        assert!(!json.contains("satellites"), "{json}");
        assert!(!json.contains("hdop"), "{json}");
    }

    #[test]
    fn a_fix_survives_the_wire() {
        let json = serde_json::to_string(&fix()).expect("serialize");

        assert_eq!(
            serde_json::from_str::<Gps<f64>>(&json).expect("read"),
            fix()
        );
    }

    /// The archive holds fixes recorded before speed and heading were sent at all.
    #[test]
    fn a_fix_from_before_speed_and_heading_were_sent_still_reads() {
        let read: Gps<f64> =
            serde_json::from_str(r#"{"lat":55.95,"lon":-3.19,"alt":null,"acc":8.5}"#)
                .expect("read");

        assert_eq!(read.speed_mps, None);
        assert_eq!(read.heading_degrees, None);
        assert_eq!(read.accuracy_metres, Some(8.5));
    }

    /// The axes are the same type, so swapping them is silent.
    #[test]
    fn a_fix_holds_its_axes_the_way_round_georust_does() {
        let read: Gps<f64> =
            serde_json::from_str(r#"{"lat":55.95,"lon":-3.19,"alt":null,"acc":8.5}"#)
                .expect("read");

        assert_eq!(read.position, Point::new(-3.19, 55.95));
        assert_eq!(read.latitude(), 55.95);
        assert_eq!(read.longitude(), -3.19);
    }

    /// A fix held at the precision the device uses. `f32` resolves a degree to about a tenth of
    /// a metre here, so the position survives the conversion at the precision a scan needs.
    #[test]
    fn a_fix_can_be_held_at_the_precision_the_device_uses() {
        let fix = Gps::<f32>::at(50.5, 8.5).expect("on the globe");

        assert!((fix.latitude() - 50.5).abs() < 1e-5);
        assert!((fix.longitude() - 8.5).abs() < 1e-5);
    }

    #[test]
    fn a_fix_off_the_globe_is_refused() {
        assert!(Gps::<f32>::at(91.0, 8.5).is_err());
    }
}
