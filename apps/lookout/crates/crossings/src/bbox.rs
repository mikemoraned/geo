//! A geographic window restricting which crossings a pack run includes.

use std::fmt::{self, Display};
use std::num::ParseFloatError;
use std::str::FromStr;

use geo_types::{Coord, Rect, coord};

/// West, south, east, north — the order Overture and the OGC use, and the order the
/// command line takes.
const CORNERS: usize = 4;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BboxError {
    #[error("expected {CORNERS} comma-separated numbers (west,south,east,north), got {0}")]
    CornerCount(usize),
    #[error("{0} is not a number")]
    NotANumber(#[from] ParseFloatError),
    #[error("latitude {0} outside -90..=90")]
    Latitude(f64),
    #[error("longitude {0} outside -180..=180")]
    Longitude(f64),
    #[error("west {west} is east of east {east}")]
    WestOfEast { west: f64, east: f64 },
    #[error("south {south} is north of north {north}")]
    SouthOfNorth { south: f64, north: f64 },
}

/// An axis-aligned lon/lat window, inclusive on every edge.
///
/// Edges are inclusive because this selects points for a dataset rather than partitioning
/// space: a crossing sitting exactly on a boundary a caller drew around a region is one they
/// meant to include.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bbox(Rect<f64>);

impl Bbox {
    pub fn new(west: f64, south: f64, east: f64, north: f64) -> Result<Self, BboxError> {
        for latitude in [south, north] {
            if !(-90.0..=90.0).contains(&latitude) {
                return Err(BboxError::Latitude(latitude));
            }
        }
        for longitude in [west, east] {
            if !(-180.0..=180.0).contains(&longitude) {
                return Err(BboxError::Longitude(longitude));
            }
        }
        if west > east {
            return Err(BboxError::WestOfEast { west, east });
        }
        if south > north {
            return Err(BboxError::SouthOfNorth { south, north });
        }

        Ok(Self(Rect::new(
            coord! { x: west, y: south },
            coord! { x: east, y: north },
        )))
    }

    pub fn contains(&self, longitude: f64, latitude: f64) -> bool {
        let (min, max) = (self.0.min(), self.0.max());
        (min.x..=max.x).contains(&longitude) && (min.y..=max.y).contains(&latitude)
    }

    pub fn min(&self) -> Coord<f64> {
        self.0.min()
    }

    pub fn max(&self) -> Coord<f64> {
        self.0.max()
    }
}

impl FromStr for Bbox {
    type Err = BboxError;

    fn from_str(window: &str) -> Result<Self, Self::Err> {
        let corners = window
            .split(',')
            .map(|corner| corner.trim().parse::<f64>())
            .collect::<Result<Vec<_>, _>>()?;

        let [west, south, east, north] = corners[..]
            .try_into()
            .map_err(|_| BboxError::CornerCount(corners.len()))?;

        Self::new(west, south, east, north)
    }
}

impl Display for Bbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (min, max) = (self.0.min(), self.0.max());
        write!(f, "{},{},{},{}", min.x, min.y, max.x, max.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The measured extent of the German crossings set.
    const GERMANY: &str = "6.08,47.42,15.04,54.93";

    #[test]
    fn a_window_parses_from_the_command_line_form() {
        let bbox: Bbox = GERMANY.parse().unwrap();

        assert_eq!(bbox.min(), coord! { x: 6.08, y: 47.42 });
        assert_eq!(bbox.max(), coord! { x: 15.04, y: 54.93 });
    }

    #[test]
    fn surrounding_whitespace_is_tolerated() {
        assert_eq!(
            " 6.08, 47.42, 15.04, 54.93 ".parse(),
            GERMANY.parse::<Bbox>()
        );
    }

    #[test]
    fn a_window_round_trips_through_its_display_form() {
        let bbox: Bbox = GERMANY.parse().unwrap();

        assert_eq!(bbox.to_string().parse(), Ok(bbox));
    }

    #[test]
    fn too_few_or_too_many_corners_are_rejected() {
        assert_eq!(
            "6.08,47.42,15.04".parse::<Bbox>(),
            Err(BboxError::CornerCount(3))
        );
        assert_eq!(
            "6.08,47.42,15.04,54.93,1".parse::<Bbox>(),
            Err(BboxError::CornerCount(5))
        );
    }

    #[test]
    fn a_non_numeric_corner_is_rejected() {
        assert!(matches!(
            "6.08,47.42,15.04,north".parse::<Bbox>(),
            Err(BboxError::NotANumber(_))
        ));
    }

    #[test]
    fn corners_outside_the_valid_ranges_are_rejected() {
        assert_eq!(
            Bbox::new(0.0, -91.0, 1.0, 1.0),
            Err(BboxError::Latitude(-91.0))
        );
        assert_eq!(
            Bbox::new(0.0, 0.0, 181.0, 1.0),
            Err(BboxError::Longitude(181.0))
        );
    }

    #[test]
    fn an_inverted_window_is_rejected() {
        assert_eq!(
            Bbox::new(15.0, 47.0, 6.0, 54.0),
            Err(BboxError::WestOfEast {
                west: 15.0,
                east: 6.0
            })
        );
        assert_eq!(
            Bbox::new(6.0, 54.0, 15.0, 47.0),
            Err(BboxError::SouthOfNorth {
                south: 54.0,
                north: 47.0
            })
        );
    }

    #[test]
    fn a_window_holds_the_points_inside_it_and_on_its_edges() {
        let bbox: Bbox = GERMANY.parse().unwrap();

        assert!(bbox.contains(13.54, 51.61));
        assert!(bbox.contains(6.08, 47.42));
        assert!(bbox.contains(15.04, 54.93));
        assert!(!bbox.contains(-0.12, 51.50));
        assert!(!bbox.contains(13.54, 55.00));
    }

    #[test]
    fn a_degenerate_window_holds_only_its_own_point() {
        let point = Bbox::new(13.54, 51.61, 13.54, 51.61).unwrap();

        assert!(point.contains(13.54, 51.61));
        assert!(!point.contains(13.55, 51.61));
    }
}
