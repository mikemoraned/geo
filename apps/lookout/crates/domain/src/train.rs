use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrainNumber(u32);

impl TrainNumber {
    pub fn from_gtfs(raw: &str) -> Option<Self> {
        raw.parse::<NonZeroU32>()
            .ok()
            .map(|number| Self(number.get()))
    }

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

    #[test]
    fn what_names_no_train_is_no_number() {
        assert_eq!(TrainNumber::from_gtfs("000000"), None);
        assert_eq!(TrainNumber::from_gtfs("0"), None);
        assert_eq!(TrainNumber::from_gtfs(""), None);
        assert_eq!(TrainNumber::from_gtfs("ICE"), None);
    }
}
