//! An axis-aligned window of degrees: the extent of what was recorded, or the region a run
//! was asked to restrict itself to.

use std::fmt::{self, Display};
use std::num::ParseFloatError;
use std::str::FromStr;

use geo::Intersects;
use geo_types::{Coord, Rect, coord};
use serde::{Deserialize, Serialize};

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
/// Edges are inclusive because this selects points rather than partitioning space: a crossing
/// sitting exactly on a boundary a caller drew around a region is one they meant to include.
///
/// A [`Rect`] underneath, which is georust's own box and orders its corners for itself, so
/// a window is never inside out whatever it was built from. What this adds is what geo has no
/// way to know: that the numbers are degrees, and the two forms a window is written in — the
/// `west,south,east,north` of a command line, and the four named corners of a stored column.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "Corners", into = "Corners")]
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

    /// The window a set of coordinates falls inside, which georust computed and which is
    /// therefore already in order.
    pub fn of(rect: Rect<f64>) -> Self {
        Self(rect)
    }

    /// The window as georust's own box, for whoever measures against it.
    pub fn rect(&self) -> Rect<f64> {
        self.0
    }

    /// Whether the window holds this position, its edges included.
    ///
    /// `intersects` rather than `contains`: georust follows the OGC there, where a rectangle
    /// contains only its interior, and a point on the boundary is one a caller who drew the
    /// boundary meant to include.
    pub fn contains(&self, longitude: f64, latitude: f64) -> bool {
        self.0.intersects(&coord! { x: longitude, y: latitude })
    }

    pub fn min(&self) -> Coord<f64> {
        self.0.min()
    }

    pub fn max(&self) -> Coord<f64> {
        self.0.max()
    }
}

/// How a window is stored: the four corners under the axis names the upstream reference data
/// uses for its own envelopes, which is what a reader of the column already expects.
#[derive(Serialize, Deserialize)]
struct Corners {
    xmin: f64,
    ymin: f64,
    xmax: f64,
    ymax: f64,
}

impl From<Corners> for Bbox {
    fn from(
        Corners {
            xmin,
            ymin,
            xmax,
            ymax,
        }: Corners,
    ) -> Self {
        Self(Rect::new(
            coord! { x: xmin, y: ymin },
            coord! { x: xmax, y: ymax },
        ))
    }
}

impl From<Bbox> for Corners {
    fn from(bbox: Bbox) -> Self {
        let (min, max) = (bbox.0.min(), bbox.0.max());
        Self {
            xmin: min.x,
            ymin: min.y,
            xmax: max.x,
            ymax: max.y,
        }
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
