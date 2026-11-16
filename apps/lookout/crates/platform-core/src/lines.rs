use domain::{Bbox, BboxError};
use geo::{Destination, Haversine};
use geo_types::Point;
use serde::{Deserialize, Serialize};
use serde_with::base64::Base64;
use serde_with::serde_as;
use wkb::error::WkbError;

use crate::app::Change;

const REACH_IN_RADII: f64 = 2.0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Encoded", into = "Encoded")]
pub struct Wkb(Vec<u8>);

impl Wkb {
    pub fn new(bytes: Vec<u8>) -> Result<Self, WkbError> {
        wkb::reader::read_wkb(&bytes)?;
        Ok(Self(bytes))
    }
}

#[serde_as]
#[derive(Serialize, Deserialize)]
struct Encoded(#[serde_as(as = "Base64")] Vec<u8>);

impl TryFrom<Encoded> for Wkb {
    type Error = WkbError;

    fn try_from(Encoded(bytes): Encoded) -> Result<Self, WkbError> {
        Self::new(bytes)
    }
}

impl From<Wkb> for Encoded {
    fn from(Wkb(bytes): Wkb) -> Self {
        Self(bytes)
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Nearby {
    asked: Option<Bbox>,
    shown: Vec<Wkb>,
}

impl Nearby {
    pub(crate) fn follow(
        &mut self,
        fix: Point<f64>,
        radius_metres: f64,
    ) -> Result<Option<Bbox>, BboxError> {
        let seen = around(fix, radius_metres)?;
        if self.asked.is_some_and(|asked| {
            asked.contains(seen.min().into()) && asked.contains(seen.max().into())
        }) {
            return Ok(None);
        }
        let area = around(fix, REACH_IN_RADII * radius_metres)?;
        self.asked = Some(area);
        Ok(Some(area))
    }

    pub(crate) fn received(&mut self, area: Bbox, lines: Vec<Wkb>) -> Change {
        if self.asked == Some(area) {
            self.shown = lines;
            Change::Moved
        } else {
            Change::Unchanged
        }
    }

    pub(crate) fn shown(&self) -> &[Wkb] {
        &self.shown
    }
}

fn around(centre: Point<f64>, metres: f64) -> Result<Bbox, BboxError> {
    let [north, east, south, west] =
        [0.0, 90.0, 180.0, 270.0].map(|bearing| Haversine.destination(centre, bearing, metres));
    Bbox::new(west.x(), south.y(), east.x(), north.y())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_in_wkb_is_taken() {
        let wkb: Result<Wkb, _> =
            serde_json::from_str(r#""AQIAAAACAAAA9ihcj8J1K0CF61G4HoVJQHsUrkfheitAZmZmZmaGSUA=""#);

        assert!(wkb.is_ok(), "{wkb:?}");
    }

    #[test]
    fn bytes_that_are_not_wkb_are_refused() {
        let wkb: Result<Wkb, _> = serde_json::from_str(r#""AQIAAAA=""#);

        assert!(wkb.is_err(), "{wkb:?}");
    }
}
