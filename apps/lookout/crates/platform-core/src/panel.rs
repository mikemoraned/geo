//! What the screen says, and what fits on it.
//!
//! A line is 13 characters of a 10-pixel font across 135 pixels, and nothing wraps: a longer
//! line runs off the side. Every field is formatted to fit, and most of what is asserted
//! below is width.

use chrono::{DateTime, Utc};
use predictor::{DEFAULT_RADIUS_METRES, Prediction};

use crate::Float;
use crate::battery::Charge;

/// Shown before the shell has reported a time.
pub(crate) const NO_TIME_YET: &str = "--:--:--";
/// Shown while the receiver has yet to produce a fix.
pub(crate) const NO_FIX_YET: &str = "no fix";
/// Shown in place of a countdown to a crossing we are not moving towards.
pub(crate) const NO_ARRIVAL: &str = "--:--";

/// How many crossings the panel has room for beneath the fix: five 20-pixel lines, ending
/// clear of the bottom of a 240-pixel screen.
pub const NEAREST_ON_SCREEN: usize = 5;

/// Every width assertion is against this. Nothing outside a test needs it, because the
/// formatting that has to fit is all here.
#[cfg(test)]
pub(crate) const CHARACTERS_PER_LINE: usize = 13;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ViewModel {
    pub clock: String,
    pub latitude: String,
    pub longitude: String,
    /// How full the battery is, drawn as bars in the corner of the first line. Empty while
    /// nothing plausible has been measured, so the panel says nothing rather than "flat".
    pub battery: String,
    /// Satellites and HDOP. The panel is the only output on a train, and without these a
    /// jittering distance cannot be told from a jittering fix.
    pub quality: String,
    /// How many crossings are inside the predictor's radius. The count and the list below it
    /// are visibly one answer.
    pub within: String,
    /// The nearest crossings, nearest first, each already formatted to fit a line.
    pub nearest: Vec<String>,
}

/// One crossing on one line: how far away it is, then how long until we reach it.
///
/// A line is too short for the crossing's id as well, and the id is the one to drop. It names
/// a row in a dataset. The distance and the countdown are the prediction.
pub(crate) fn line(prediction: &Prediction<Float>, now: Option<DateTime<Utc>>) -> String {
    format!(
        "{} {}",
        distance(prediction.metres),
        countdown(prediction.at, now)
    )
}

/// The battery as bars in brackets, one for each step of [`Charge`].
///
/// Five characters, which is what is left of the first line once the clock has taken eight of
/// thirteen. `FONT_10X20` is an ASCII font, so a battery glyph is not an option.
pub(crate) fn bars(charge: Charge) -> &'static str {
    const FILLED: [&str; Charge::BARS + 1] = ["[   ]", "[=  ]", "[== ]", "[===]"];

    FILLED[charge.bars()]
}

/// How many crossings the predictor reports, and how far out it looked.
pub(crate) fn within(count: usize) -> String {
    format!("{count} in {:.0}km", DEFAULT_RADIUS_METRES / 1_000.0)
}

/// A distance in six characters at most: metres up to a kilometre, then kilometres, and past
/// a thousand of those only that it is a long way.
fn distance(metres: Float) -> String {
    match metres {
        metres if metres < 1_000.0 => format!("{metres:.0}m"),
        metres if metres < 100_000.0 => format!("{:.1}km", metres / 1_000.0),
        metres if metres < 1_000_000.0 => format!("{:.0}km", metres / 1_000.0),
        _ => ">999km".to_string(),
    }
}

/// How long until we arrive, never more than five characters.
///
/// A prediction carries an instant rather than a countdown, so it stays true while the clock
/// advances between fixes. This is where the clock is subtracted from it. A standstill, or a
/// fix with no speed behind it, leaves nothing to count down to and reads as [`NO_ARRIVAL`].
fn countdown(at: Option<DateTime<Utc>>, now: Option<DateTime<Utc>>) -> String {
    let (Some(at), Some(now)) = (at, now) else {
        return NO_ARRIVAL.to_string();
    };

    match (at - now).num_seconds() {
        // Further ahead than a crossing inside the radius can honestly be, and hours would
        // not fit beside the distance anyway.
        seconds if seconds >= 3_600 => ">1h".to_string(),
        // Past its arrival: the next fix moves it, and until then it is as near as it gets.
        seconds if seconds <= 0 => "0:00".to_string(),
        seconds => format!("{}:{:02}", seconds / 60, seconds % 60),
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeDelta, TimeZone};
    use predictor::CrossingId;

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

    /// Standing still there is a distance and no arrival, because we never arrive.
    #[test]
    fn a_crossing_we_are_not_moving_towards_counts_down_to_nothing() {
        let now = instant();

        assert_eq!(countdown(None, Some(now)), NO_ARRIVAL);
        assert_eq!(countdown(Some(now), None), NO_ARRIVAL);
    }

    /// Every band the distance has a format for, against every band the countdown has one for.
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
                        crossing: CrossingId::new(u32::MAX),
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
                crossing: CrossingId::new(u32::MAX),
                metres: 999_999.0,
                at: None,
            },
            Some(instant()),
        );

        assert!(line.chars().count() <= CHARACTERS_PER_LINE, "{line:?}");
    }

    /// However many are near, the count fits. The whole carried set is the bound, which no
    /// radius this small reaches, so a line surviving it survives anything real.
    #[test]
    fn the_count_fits_however_many_are_near() {
        let widest = within(crate::carried::crossings().len());

        assert!(widest.chars().count() <= CHARACTERS_PER_LINE, "{widest:?}");
    }

    /// The battery shares the first line with the clock: eight characters of eight-plus-five.
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
