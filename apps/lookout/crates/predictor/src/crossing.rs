//! The points a predictor scans against.

use geo_types::Point;
use serde::{Deserialize, Serialize};

use crate::measure::Measure;
use crate::sample::{CoordinateError, position};

/// Identifies one crossing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CrossingId(u32);

impl CrossingId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    pub fn value(&self) -> u32 {
        self.0
    }
}

/// One crossing: which one it is, and where.
///
/// It measures in the same float a [`crate::Sample`] does, since the two are subtracted from
/// each other. On the device that is `f32`, which is what makes a scan of the whole set
/// against every fix affordable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing<T: Measure> {
    pub id: CrossingId,
    /// Degrees, longitude in `x` and latitude in `y`.
    pub position: Point<T>,
}

impl<T: Measure> Crossing<T> {
    pub fn new(id: CrossingId, position: Point<T>) -> Self {
        Self { id, position }
    }

    /// The same from degrees, latitude first, as [`crate::Sample::at`] takes them.
    ///
    /// Checked on the same terms as a sample's position. The set is read from a flash buffer
    /// that can arrive corrupt, and an unchecked crossing off the globe is scanned against
    /// every fix instead of failing once, here.
    pub fn at(
        id: u32,
        latitude_degrees: f64,
        longitude_degrees: f64,
    ) -> Result<Self, CoordinateError> {
        Ok(Self::new(
            CrossingId::new(id),
            position(latitude_degrees, longitude_degrees)?,
        ))
    }

    pub fn latitude(&self) -> T {
        self.position.y()
    }

    pub fn longitude(&self) -> T {
        self.position.x()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The axes are the same type, so swapping them is silent.
    #[test]
    fn a_crossing_holds_its_axes_the_way_round_georust_does() {
        let crossing = Crossing::<f64>::at(7, 51.5, 13.5).expect("on the globe");

        assert_eq!(crossing.position, Point::new(13.5, 51.5));
        assert_eq!(crossing.latitude(), 51.5);
        assert_eq!(crossing.longitude(), 13.5);
        assert_eq!(crossing.id.value(), 7);
    }

    #[test]
    fn a_crossing_off_the_globe_is_refused() {
        assert!(Crossing::<f32>::at(7, 91.0, 13.5).is_err());
    }
}
