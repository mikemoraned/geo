//! A crossing: what it is called, and where it is.
//!
//! A crossing has two names, because the places that hold one have different room for it.
//! [`CrossingId`] says what the crossing is made of, and is what a store, a query or a person
//! reads. [`CrossingCompactId`] is the same crossing in four bytes, which is what a device has
//! space for beside a coordinate. Both are names for one crossing, and both are defined here,
//! so whoever holds a crossing picks the name its room allows rather than spelling out one of
//! its own.
//!
//! [`CrossingCompact`] is the crossing where space is not free: named in four bytes, measured
//! in whatever float the holder scans in, and sent as three numbers rather than three named
//! fields. Those are one decision, not three, which is why they are one type.

use std::fmt::{self, Display};
use std::str::FromStr;

use geo_types::Point;
use serde::{Deserialize, Serialize};

use crate::position::{CoordinateError, degrees, position};
use crate::precision::Precision;

/// A name that could not be written as one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is empty or holds a character a name cannot be written with")]
pub struct CrossingIdError(String);

/// Characters an id cannot hold, because whatever writes one out — a directory a store
/// partitions by, a path, a query — would read them as punctuation rather than as the name.
const RESERVED: [char; 3] = ['/', ' ', '='];

/// Identifies one crossing.
///
/// Derived from what the crossing *is* — the water, the stretch of track, and where along that
/// track the two meet — so a ground truth recorded by one run and a prediction made by another
/// refer to the same crossing, and a rerun over the same reference data lands on the same ids.
///
/// The place is part of the identity because one track crosses one body of water more than
/// once: a line following a valley crosses the river beside it repeatedly, and those are
/// separate sightings rather than one.
///
/// It is a name and stays writable as one: an id reaches a store that lays data out in
/// directories named by it, and a URL that asks for one, so the characters those would misread
/// are refused here rather than where a path is built.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CrossingId(String);

impl CrossingId {
    pub fn new(id: impl Into<String>) -> Result<Self, CrossingIdError> {
        let id = id.into();
        if id.is_empty() || id.contains(RESERVED) {
            Err(CrossingIdError(id))
        } else {
            Ok(Self(id))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for CrossingId {
    type Err = CrossingIdError;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::new(id)
    }
}

impl Display for CrossingId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The same crossing named in four bytes, which is what a device has room for beside a
/// coordinate.
///
/// Minted from [`CrossingId`] where the crossing is derived, which is also where two crossings
/// landing on one of these is refused, so nothing downstream derives one: a packer, a scan and
/// a page all read the name they were given.
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

/// Written as the four bytes it is, so two ids are the same width however small the numbers.
impl Display for CrossingCompactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

/// One crossing: which one it is, and where.
///
/// Degrees in `f64`, and the name that says what the crossing is made of, which is what
/// anything with room for it holds. [`CrossingCompact`] is the same crossing where there is
/// not room. Whatever a derivation needs beyond this — the extraction it came from, the same
/// place projected into metres — it adds beside one of these rather than restating the two
/// fields every crossing has.
///
/// It is not serialised. What goes over a wire or into a buffer is [`CrossingCompact`], which
/// is sized for that; this is what a derivation holds while it works.
#[derive(Debug, Clone, PartialEq)]
pub struct Crossing {
    pub id: CrossingId,
    /// Degrees, longitude in `x` and latitude in `y`.
    pub position: Point<f64>,
}

impl Crossing {
    pub fn new(id: CrossingId, position: Point<f64>) -> Self {
        Self { id, position }
    }

    /// A crossing from degrees, latitude first.
    ///
    /// # Errors
    ///
    /// Returns an error where the coordinates are not on the globe.
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

/// One crossing where space is not free: which one it is, and where.
///
/// Measured in the float the platform holding it works in, since a scan subtracts a crossing
/// from a fix. On the device that is `f32`, which is what makes scanning the whole set against
/// every fix affordable; a source reports degrees in `f64`, and that is what a set is sent in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "CrossingCompactRow", into = "CrossingCompactRow", bound = "")]
pub struct CrossingCompact<P: Precision> {
    pub id: CrossingCompactId,
    /// Degrees, longitude in `x` and latitude in `y`.
    pub position: Point<P>,
}

impl<P: Precision> CrossingCompact<P> {
    pub fn new(id: impl Into<CrossingCompactId>, position: Point<P>) -> Self {
        Self {
            id: id.into(),
            position,
        }
    }

    /// A crossing from degrees, latitude first.
    ///
    /// Checked on the same terms as a fix. A set is read from a flash buffer that can arrive
    /// corrupt, or fetched over a network, and an unchecked crossing off the globe is scanned
    /// against every fix instead of failing once, here.
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

/// How a compact crossing is sent: `[id, latitude, longitude]`.
///
/// A set of thousands goes as one array, and this is a third the size of the same thing with
/// its field names repeated on every row. Latitude leads, as it does everywhere a coordinate
/// is written here in degrees, which is the opposite order to the `x`, `y` it is held in.
///
/// Nothing is checked on the way in: a set arrives with whatever a source put in it, and
/// whoever reads it drops the rows it cannot use rather than losing the set to one of them.
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

    /// The array leads with latitude and the point holds longitude first, so a swap between
    /// the two would put every crossing somewhere else entirely.
    #[test]
    fn reading_one_back_keeps_latitude_and_longitude_apart() {
        let read: CrossingCompact<f64> = serde_json::from_str("[7,51.0403,13.7322]").expect("read");

        assert_eq!(read.latitude(), 51.0403);
        assert_eq!(read.longitude(), 13.7322);
        assert_eq!(read.position.x(), 13.7322);
    }

    /// The axes are the same type, so swapping them is silent.
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

    /// An id is written out as a directory name and asked for in a URL, so one holding what
    /// either would misread is refused where it is made rather than where it is used.
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

    /// The four bytes a device holds, written the width they are.
    #[test]
    fn a_compact_id_reads_as_the_four_bytes_it_is() {
        assert_eq!(CrossingCompactId::new(0x2620_a981).to_string(), "2620a981");
        assert_eq!(CrossingCompactId::new(7).to_string(), "00000007");
    }
}
