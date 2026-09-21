//! A train, as a timetable names one.

use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

/// A train's operating number — the integer GTFS `trip_short_name`, e.g. `2569` from the
/// raw `002569`. Distinct from the *line* (`route_short_name`, e.g. `55`) and from `mode`,
/// which already carries the product family. Non-zero: `0` is not a real train number.
///
/// Held as a plain `u32` rather than a `NonZeroU32`, though zero is what it refuses: a
/// schema is traced by probing the type with values, zero among them, so a number that
/// cannot be zero cannot describe a column. What keeps a zero out of one is this being the
/// only way to make one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrainNumber(u32);

impl TrainNumber {
    /// From a raw GTFS `trip_short_name`; `None` when it names no number — empty, all zeros
    /// (some long-distance trips report `0` / `000000`), or non-numeric. Leading zeros are
    /// dropped by the integer parse.
    ///
    /// An `Option` and not a `Result`, and no `FromStr` beside it: a trip that names no
    /// train is ordinary rather than a failure to read one, and nothing downstream needs a
    /// number. A parse that erred would be turned straight back into this by every caller.
    pub fn from_gtfs(raw: &str) -> Option<Self> {
        raw.parse::<NonZeroU32>()
            .ok()
            .map(|number| Self(number.get()))
    }

    /// The number, e.g. `2569`.
    pub fn get(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_drops_the_leading_zeros_a_timetable_pads_with() {
        assert_eq!(
            TrainNumber::from_gtfs("002569").map(TrainNumber::get),
            Some(2569)
        );
        assert_eq!(
            TrainNumber::from_gtfs("2569").map(TrainNumber::get),
            Some(2569)
        );
    }

    /// Some long-distance trips report no number, as zeros or as nothing at all. A train
    /// numbered zero is not a train, so it is absent rather than stored as one.
    #[test]
    fn what_names_no_train_is_no_number() {
        assert_eq!(TrainNumber::from_gtfs("000000"), None);
        assert_eq!(TrainNumber::from_gtfs("0"), None);
        assert_eq!(TrainNumber::from_gtfs(""), None);
        assert_eq!(TrainNumber::from_gtfs("ICE"), None);
    }
}
