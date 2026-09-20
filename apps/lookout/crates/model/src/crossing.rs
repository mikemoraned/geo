//! A crossing, as one is sent to whoever predicts against it.

use geo_types::Point;
use serde::{Deserialize, Serialize};

/// One crossing: which one it is, and where, in the degrees its source reported.
///
/// [`predictor::Crossing`] is the same crossing held in the float a scan measures in. The two
/// exist separately because this one is what a source says and that one is what arithmetic
/// needs; whether they should stay separate is the question `docs/current-slice.md` defers to
/// its last phase.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "Row", into = "Row")]
pub struct Crossing {
    pub id: u32,
    /// Degrees, longitude in `x` and latitude in `y`.
    pub position: Point<f64>,
}

impl Crossing {
    pub fn new(id: u32, latitude: f64, longitude: f64) -> Self {
        Self {
            id,
            position: Point::new(longitude, latitude),
        }
    }

    pub fn latitude(&self) -> f64 {
        self.position.y()
    }

    pub fn longitude(&self) -> f64 {
        self.position.x()
    }
}

/// How a crossing is sent: `[id, latitude, longitude]`.
///
/// A set of thousands goes as one array, and this is a third the size of the same thing with
/// its field names repeated on every row. Latitude leads, as it does everywhere a coordinate
/// is written here in degrees, which is the opposite order to the `x`, `y` it is held in.
#[derive(Serialize, Deserialize)]
struct Row(u32, f64, f64);

impl From<Row> for Crossing {
    fn from(Row(id, latitude, longitude): Row) -> Self {
        Self::new(id, latitude, longitude)
    }
}

impl From<Crossing> for Row {
    fn from(crossing: Crossing) -> Self {
        Self(crossing.id, crossing.latitude(), crossing.longitude())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crossing_is_sent_as_an_array_of_id_then_degrees() {
        let json = serde_json::to_string(&[Crossing::new(7, 51.0403, 13.7322)]).expect("write");

        assert_eq!(json, "[[7,51.0403,13.7322]]");
    }

    #[test]
    fn a_crossing_survives_being_sent() {
        let one = Crossing::new(0x2620_a981, 51.0403, 13.7322);

        let json = serde_json::to_string(&one).expect("write");

        assert_eq!(serde_json::from_str::<Crossing>(&json).expect("read"), one);
    }

    /// The array leads with latitude and the point holds longitude first, so a swap between
    /// the two would put every crossing somewhere else entirely.
    #[test]
    fn reading_one_back_keeps_latitude_and_longitude_apart() {
        let read: Crossing = serde_json::from_str("[7,51.0403,13.7322]").expect("read");

        assert_eq!(read.latitude(), 51.0403);
        assert_eq!(read.longitude(), 13.7322);
        assert_eq!(read.position.x(), 13.7322);
    }
}
