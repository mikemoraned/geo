use geo_types::Point;
use serde::{Deserialize, Serialize, Serializer, de::Deserializer};

use crate::position::{CoordinateError, degrees, position};
use crate::precision::Precision;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gps<P: Precision> {
    pub position: Point<P>,
    pub altitude_metres: Option<P>,
    pub accuracy_metres: Option<P>,
    pub speed_mps: Option<P>,
    pub heading_degrees: Option<P>,
    pub satellites: Option<u32>,
    pub hdop: Option<P>,
}

impl<P: Precision> Gps<P> {
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

    pub fn at(latitude_degrees: f64, longitude_degrees: f64) -> Result<Self, CoordinateError> {
        Ok(Self::new(position(latitude_degrees, longitude_degrees)?))
    }

    pub fn latitude(&self) -> P {
        self.position.y()
    }

    pub fn longitude(&self) -> P {
        self.position.x()
    }

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

fn held<P: Precision>(value: Option<f64>) -> Option<P> {
    value.and_then(P::from_f64)
}

fn reported<P: Precision>(value: P) -> f64 {
    value.to_f64().expect("a degree is a number")
}

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

    #[test]
    fn a_fix_is_written_in_the_names_the_wire_uses() {
        let json = serde_json::to_string(&fix()).expect("serialize");

        assert_eq!(
            json,
            r#"{"lat":55.95,"lon":-3.19,"alt":null,"acc":8.5,"speed":31.4,"heading":null}"#
        );
    }

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

    #[test]
    fn a_fix_from_before_speed_and_heading_were_sent_still_reads() {
        let read: Gps<f64> =
            serde_json::from_str(r#"{"lat":55.95,"lon":-3.19,"alt":null,"acc":8.5}"#)
                .expect("read");

        assert_eq!(read.speed_mps, None);
        assert_eq!(read.heading_degrees, None);
        assert_eq!(read.accuracy_metres, Some(8.5));
    }

    #[test]
    fn a_fix_holds_its_axes_the_way_round_georust_does() {
        let read: Gps<f64> =
            serde_json::from_str(r#"{"lat":55.95,"lon":-3.19,"alt":null,"acc":8.5}"#)
                .expect("read");

        assert_eq!(read.position, Point::new(-3.19, 55.95));
        assert_eq!(read.latitude(), 55.95);
        assert_eq!(read.longitude(), -3.19);
    }

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
