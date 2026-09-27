use chrono::{DateTime, Utc};
use predictor::{DEFAULT_RADIUS_METRES, Prediction};

use platform_core::Float;
use platform_core::battery::Charge;

pub(crate) const NO_TIME_YET: &str = "--:--:--";
pub(crate) const NO_FIX_YET: &str = "no fix";
pub(crate) const NO_ARRIVAL: &str = "--:--";
const ALREADY_DUE: &str = "0:00";
const LONGER_THAN_FITS: i64 = 3_600;

pub const NEAREST_ON_SCREEN: usize = 5;

#[cfg(test)]
pub(crate) const CHARACTERS_PER_LINE: usize = 13;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ViewModel {
    pub clock: String,
    pub latitude: String,
    pub longitude: String,
    pub battery: String,
    pub quality: String,
    pub within: String,
    pub nearest: Vec<String>,
}

pub(crate) fn line(prediction: &Prediction<Float>, now: Option<DateTime<Utc>>) -> String {
    format!(
        "{} {}",
        distance(prediction.metres),
        countdown(prediction.at, now)
    )
}

pub(crate) fn bars(charge: Charge) -> &'static str {
    const FILLED: [&str; Charge::BARS + 1] = ["[   ]", "[=  ]", "[== ]", "[===]"];

    FILLED[charge.bars()]
}

pub(crate) fn within(count: usize) -> String {
    format!("{count} in {:.0}km", DEFAULT_RADIUS_METRES / 1_000.0)
}

fn distance(metres: Float) -> String {
    match metres {
        metres if metres < 1_000.0 => format!("{metres:.0}m"),
        metres if metres < 100_000.0 => format!("{:.1}km", metres / 1_000.0),
        metres if metres < 1_000_000.0 => format!("{:.0}km", metres / 1_000.0),
        _ => ">999km".to_string(),
    }
}

fn countdown(at: Option<DateTime<Utc>>, now: Option<DateTime<Utc>>) -> String {
    let (Some(at), Some(now)) = (at, now) else {
        return NO_ARRIVAL.to_string();
    };

    match (at - now).num_seconds() {
        seconds if seconds >= LONGER_THAN_FITS => ">1h".to_string(),
        seconds if seconds <= 0 => ALREADY_DUE.to_string(),
        seconds => format!("{}:{:02}", seconds / 60, seconds % 60),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeDelta, TimeZone};
    use domain::CrossingCompactId;

    use super::*;

    fn instant() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 29, 20, 43, 29)
            .single()
            .expect("an instant")
    }

    #[test]
    fn a_distance_is_said_in_the_unit_that_suits_it() {
        assert_eq!(distance(0.0), "0m");
        assert_eq!(distance(942.0), "942m");
        assert_eq!(distance(1_500.0), "1.5km");
        assert_eq!(distance(250_000.0), "250km");
        assert_eq!(distance(20_015_000.0), ">999km");
    }

    #[test]
    fn a_countdown_is_minutes_and_seconds_until_it_is_too_far_off_to_be() {
        let now = instant();
        let counting = |seconds: i64| countdown(Some(now + TimeDelta::seconds(seconds)), Some(now));

        assert_eq!(counting(0), "0:00");
        assert_eq!(counting(-30), "0:00");
        assert_eq!(counting(9), "0:09");
        assert_eq!(counting(84), "1:24");
        assert_eq!(counting(3_599), "59:59");
        assert_eq!(counting(3_600), ">1h");
    }

    #[test]
    fn a_crossing_we_are_not_moving_towards_counts_down_to_nothing() {
        let now = instant();

        assert_eq!(countdown(None, Some(now)), NO_ARRIVAL);
        assert_eq!(countdown(Some(now), None), NO_ARRIVAL);
    }

    #[test]
    fn no_prediction_can_make_a_line_too_long() {
        let now = instant();

        for metres in [
            0.0,
            999.4,
            999.6,
            1_000.0,
            99_949.0,
            100_000.0,
            999_999.0,
            20_015_000.0,
        ] {
            for seconds in [-1, 0, 1, 59, 60, 599, 600, 3_599, 3_600, 86_400] {
                let line = line(
                    &Prediction {
                        crossing_compact_id: CrossingCompactId::new(u32::MAX),
                        metres,
                        at: Some(now + TimeDelta::seconds(seconds)),
                    },
                    Some(now),
                );
                assert!(
                    line.chars().count() <= CHARACTERS_PER_LINE,
                    "{line:?} is {} characters at {metres}m and {seconds}s",
                    line.chars().count(),
                );
            }
        }
    }

    #[test]
    fn a_line_with_no_countdown_still_fits() {
        let line = line(
            &Prediction {
                crossing_compact_id: CrossingCompactId::new(u32::MAX),
                metres: 999_999.0,
                at: None,
            },
            Some(instant()),
        );

        assert!(line.chars().count() <= CHARACTERS_PER_LINE, "{line:?}");
    }

    #[test]
    fn the_count_fits_however_many_are_near() {
        let widest = within(crate::carried::crossings().len());

        assert!(widest.chars().count() <= CHARACTERS_PER_LINE, "{widest:?}");
    }

    #[test]
    fn the_battery_fits_beside_the_clock() {
        assert_eq!(
            NO_TIME_YET.chars().count() + bars(Charge::Full).chars().count(),
            CHARACTERS_PER_LINE
        );
    }

    #[test]
    fn each_step_fills_one_more_bar_than_the_last() {
        assert_eq!(bars(Charge::Empty), "[   ]");
        assert_eq!(bars(Charge::Half), "[== ]");
        assert_eq!(bars(Charge::Full), "[===]");
    }
}
