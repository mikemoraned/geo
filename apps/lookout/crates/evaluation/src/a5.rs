use std::fmt;

use a5::{LonLat, MAX_RESOLUTION};
use geo_types::Point;
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

impl fmt::Display for A5CellId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&a5::u64_to_hex(self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
