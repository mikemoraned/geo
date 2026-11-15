use std::collections::BTreeSet;
use std::fmt;

use a5::{Containment, LonLat, MAX_RESOLUTION, PolygonToCellsOptions};
use geo_types::{Point, Polygon};
use hashfn::hashfn;
use nutype::nutype;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct A5CellId(pub u64);

#[nutype(
    validate(with = a5_resolution_in_range, error = NotAnA5Resolution),
    derive(
        Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Display, FromStr, TryFrom, Into
    )
)]
pub struct A5Resolution(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{0} is not an A5 resolution, from 0 to 30")]
pub struct NotAnA5Resolution(pub u8);

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("no cell at resolution {resolution} holds {point:?}: {reason}")]
pub struct NoA5Cell {
    pub point: Point<f64>,
    pub resolution: A5Resolution,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A5Cover {
    cells: BTreeSet<A5CellId>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("no cover at resolution {resolution}: {reason}")]
pub struct NoA5Cover {
    pub resolution: A5Resolution,
    pub reason: String,
}

#[hashfn]
pub fn a5_resolution_in_range(resolution: &u8) -> Result<(), NotAnA5Resolution> {
    if i32::from(*resolution) <= MAX_RESOLUTION {
        Ok(())
    } else {
        Err(NotAnA5Resolution(*resolution))
    }
}

impl A5Resolution {
    pub fn new(resolution: u8) -> Result<Self, NotAnA5Resolution> {
        Self::try_new(resolution)
    }
}

impl A5CellId {
    pub fn containing(point: Point<f64>, resolution: A5Resolution) -> Result<Self, NoA5Cell> {
        a5::lonlat_to_cell(
            LonLat::new(point.x(), point.y()),
            i32::from(u8::from(resolution)),
        )
        .map(Self)
        .map_err(|reason| NoA5Cell {
            point,
            resolution,
            reason,
        })
    }
}

impl A5Cover {
    pub fn overlapping<'a>(
        areas: impl IntoIterator<Item = &'a Polygon<f64>>,
        resolution: A5Resolution,
    ) -> Result<Self, NoA5Cover> {
        let no_cover = |reason| NoA5Cover { resolution, reason };
        let options = PolygonToCellsOptions {
            containment: Containment::Overlapping,
        };
        let cells = areas
            .into_iter()
            .map(|area| {
                a5::polygon_to_cells(
                    &rings_of(area),
                    i32::from(u8::from(resolution)),
                    Some(options),
                )
                .and_then(|compacted| a5::uncompact(&compacted))
                .map_err(no_cover)
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .map(A5CellId)
            .collect();
        Ok(Self { cells })
    }

    pub fn contains(&self, cell: A5CellId) -> bool {
        self.cells.contains(&cell)
    }
}

fn rings_of(area: &Polygon<f64>) -> Vec<Vec<LonLat>> {
    std::iter::once(area.exterior())
        .chain(area.interiors())
        .map(|ring| {
            ring.coords()
                .map(|coord| LonLat::new(coord.x, coord.y))
                .collect()
        })
        .collect()
}

impl fmt::Display for A5CellId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&a5::u64_to_hex(self.0))
    }
}

#[cfg(test)]
mod tests {
    use geo_types::polygon;
    use proptest::prelude::*;

    use super::*;

    fn resolution() -> A5Resolution {
        A5Resolution::new(8).unwrap()
    }

    fn around_dresden() -> Polygon<f64> {
        polygon![
            (x: 13.2, y: 50.5),
            (x: 14.2, y: 50.5),
            (x: 14.2, y: 51.5),
            (x: 13.2, y: 51.5),
        ]
    }

    #[test]
    fn a_cover_holds_the_cell_of_a_point_inside_its_area() {
        let cover = A5Cover::overlapping([&around_dresden()], resolution()).unwrap();

        let dresden_hbf = A5CellId::containing(Point::new(13.7322, 51.0403), resolution()).unwrap();
        assert!(cover.contains(dresden_hbf));
    }

    #[test]
    fn a_cover_leaves_out_the_cell_of_a_point_far_outside_its_area() {
        let cover = A5Cover::overlapping([&around_dresden()], resolution()).unwrap();

        let berlin_hbf = A5CellId::containing(Point::new(13.3695, 52.5251), resolution()).unwrap();
        assert!(!cover.contains(berlin_hbf));
    }

    proptest! {
        #[test]
        fn every_point_inside_an_area_lies_in_a_cell_of_its_cover(
            lon in 13.2..14.2f64,
            lat in 50.5..51.5f64,
        ) {
            let cover = A5Cover::overlapping([&around_dresden()], resolution()).unwrap();

            let cell = A5CellId::containing(Point::new(lon, lat), resolution()).unwrap();
            prop_assert!(cover.contains(cell));
        }
    }

    #[test]
    fn dresden_hbf_lies_in_one_cell_at_resolution_8() {
        let cell =
            A5CellId::containing(Point::new(13.7322, 51.0403), A5Resolution::new(8).unwrap())
                .unwrap();

        assert_eq!(cell.to_string(), "5156880000000000");
    }

    #[test]
    fn a_resolution_runs_from_0_to_30() {
        assert_eq!(A5Resolution::new(0).map(u8::from), Ok(0));
        assert_eq!(A5Resolution::new(30).map(u8::from), Ok(30));
        assert_eq!(A5Resolution::new(31), Err(NotAnA5Resolution(31)));
        assert!("-1".parse::<A5Resolution>().is_err());
    }
}
