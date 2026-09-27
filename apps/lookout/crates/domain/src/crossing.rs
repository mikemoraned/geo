use std::fmt::{self, Display};
use std::str::FromStr;

use geo_types::Point;
use serde::{Deserialize, Serialize};

use crate::name::{NameError, checked};
use crate::position::{CoordinateError, degrees, position};
use crate::precision::Precision;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CrossingId(String);

impl CrossingId {
    pub fn new(id: impl Into<String>) -> Result<Self, NameError> {
        Ok(Self(checked(id.into())?))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for CrossingId {
    type Err = NameError;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::new(id)
    }
}

impl Display for CrossingId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct CrossingCompactId(u32);

impl CrossingCompactId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    pub fn get(&self) -> u32 {
        self.0
    }
}

impl From<u32> for CrossingCompactId {
    fn from(id: u32) -> Self {
        Self::new(id)
    }
}

impl Display for CrossingCompactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Crossing {
    pub id: CrossingId,
    pub position: Point<f64>,
}

impl Crossing {
    pub fn new(id: CrossingId, position: Point<f64>) -> Self {
        Self { id, position }
    }

    pub fn at(
        id: CrossingId,
        latitude_degrees: f64,
        longitude_degrees: f64,
    ) -> Result<Self, CoordinateError> {
        Ok(Self::new(
            id,
            position(latitude_degrees, longitude_degrees)?,
        ))
    }

    pub fn latitude(&self) -> f64 {
        self.position.y()
    }

    pub fn longitude(&self) -> f64 {
        self.position.x()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "CrossingCompactRow", into = "CrossingCompactRow", bound = "")]
pub struct CrossingCompact<P: Precision> {
    pub id: CrossingCompactId,
    pub position: Point<P>,
}

impl<P: Precision> CrossingCompact<P> {
    pub fn new(id: impl Into<CrossingCompactId>, position: Point<P>) -> Self {
        Self {
            id: id.into(),
            position,
        }
    }

    pub fn at(
        id: impl Into<CrossingCompactId>,
        latitude_degrees: f64,
        longitude_degrees: f64,
    ) -> Result<Self, CoordinateError> {
        Ok(Self::new(
            id,
            position(latitude_degrees, longitude_degrees)?,
        ))
    }

    pub fn latitude(&self) -> P {
        self.position.y()
    }

    pub fn longitude(&self) -> P {
        self.position.x()
    }
}

#[derive(Serialize, Deserialize)]
struct CrossingCompactRow(CrossingCompactId, f64, f64);

impl<P: Precision> From<CrossingCompactRow> for CrossingCompact<P> {
    fn from(CrossingCompactRow(id, latitude, longitude): CrossingCompactRow) -> Self {
        Self::new(id, Point::new(degrees(longitude), degrees(latitude)))
    }
}

impl<P: Precision> From<CrossingCompact<P>> for CrossingCompactRow {
    fn from(crossing: CrossingCompact<P>) -> Self {
        Self(
            crossing.id,
            crossing.latitude().to_f64().expect("a degree is a number"),
            crossing.longitude().to_f64().expect("a degree is a number"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crossing_is_sent_as_an_array_of_id_then_degrees() {
        let json =
            serde_json::to_string(&[CrossingCompact::<f64>::new(7, Point::new(13.7322, 51.0403))])
                .expect("write");

        assert_eq!(json, "[[7,51.0403,13.7322]]");
    }

    #[test]
    fn a_crossing_survives_being_sent() {
        let one = CrossingCompact::<f64>::at(0x2620_a981, 51.0403, 13.7322).expect("on the globe");

        let json = serde_json::to_string(&one).expect("write");

        assert_eq!(
            serde_json::from_str::<CrossingCompact<f64>>(&json).expect("read"),
            one
        );
    }

    #[test]
    fn reading_one_back_keeps_latitude_and_longitude_apart() {
        let read: CrossingCompact<f64> = serde_json::from_str("[7,51.0403,13.7322]").expect("read");

        assert_eq!(read.latitude(), 51.0403);
        assert_eq!(read.longitude(), 13.7322);
        assert_eq!(read.position.x(), 13.7322);
    }

    #[test]
    fn a_crossing_holds_its_axes_the_way_round_georust_does() {
        let crossing = CrossingCompact::<f32>::at(7, 51.5, 13.5).expect("on the globe");

        assert_eq!(crossing.position, Point::new(13.5, 51.5));
        assert_eq!(crossing.latitude(), 51.5);
        assert_eq!(crossing.longitude(), 13.5);
        assert_eq!(crossing.id.get(), 7);
    }

    #[test]
    fn a_crossing_off_the_globe_is_refused() {
        assert!(CrossingCompact::<f32>::at(7, 91.0, 13.5).is_err());
    }

    #[test]
    fn an_id_that_could_not_be_written_as_a_name_is_refused() {
        assert!(CrossingId::new("water/track").is_err());
        assert!(CrossingId::new("water track").is_err());
        assert!(CrossingId::new("water=track").is_err());
        assert!(CrossingId::new("").is_err());
        assert_eq!(
            "08b2a5c1fffffff-08f2a5c1"
                .parse::<CrossingId>()
                .expect("a name")
                .to_string(),
            "08b2a5c1fffffff-08f2a5c1"
        );
    }

    #[test]
    fn a_crossing_holds_the_name_and_the_place_every_crossing_has() {
        let id: CrossingId = "water:track:rail@0.5".parse().expect("a name");

        let crossing = Crossing::at(id.clone(), 51.5, 13.5).expect("on the globe");

        assert_eq!(crossing.id, id);
        assert_eq!(crossing.latitude(), 51.5);
        assert_eq!(crossing.longitude(), 13.5);
        assert_eq!(crossing.position, Point::new(13.5, 51.5));
    }

    #[test]
    fn a_compact_id_reads_as_the_four_bytes_it_is() {
        assert_eq!(CrossingCompactId::new(0x2620_a981).to_string(), "2620a981");
        assert_eq!(CrossingCompactId::new(7).to_string(), "00000007");
    }
}
