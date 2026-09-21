//! A fix, and when it was taken.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::gps::Gps;

/// One reported fix: what a source knew about where it was, and the instant it knew it for.
///
/// The instant is the reading's, not the moment anything received it. A browser's
/// `watchPosition` fix is seconds old by the time it arrives, and a replayed one is months
/// old, so whoever handles a sample reads the time from the sample rather than from a clock.
///
/// [`predictor::Sample`] is the same fix in the float a scan measures in, carrying what a
/// receiver additionally reports. This is the reported one, in the degrees every source hands
/// over.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub t: DateTime<Utc>,
    pub gps: Gps,
}

impl Sample {
    pub fn new(t: DateTime<Utc>, gps: Gps) -> Self {
        Self { t, gps }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Sample {
        Sample::new(
            DateTime::from_timestamp(1_785_098_609, 0).expect("an instant"),
            Gps {
                latitude: 51.0403,
                longitude: 13.7322,
                altitude_metres: None,
                accuracy_metres: 5.0,
                speed_mps: Some(27.8),
                heading_degrees: None,
            },
        )
    }

    /// The shape a kiosk replays and a shell dispatches, written once here rather than at each
    /// end: a page sends what it read, and reshapes nothing.
    #[test]
    fn a_sample_is_written_as_an_instant_beside_the_fix() {
        let json = serde_json::to_string(&sample()).expect("write");

        assert_eq!(
            json,
            r#"{"t":"2026-07-26T20:43:29Z","gps":{"lat":51.0403,"lon":13.7322,"alt":null,"acc":5.0,"speed":27.8,"heading":null}}"#
        );
    }

    #[test]
    fn a_sample_survives_being_sent() {
        let json = serde_json::to_string(&sample()).expect("write");

        assert_eq!(
            serde_json::from_str::<Sample>(&json).expect("read"),
            sample()
        );
    }
}
