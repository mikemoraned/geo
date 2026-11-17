use std::fmt::{self, Display};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct RealtimeShare(f64);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a share, a number from 0 to 1")]
pub struct NotAShare(String);

impl RealtimeShare {
    pub fn new(share: f64) -> Result<Self, NotAShare> {
        if (0.0..=1.0).contains(&share) {
            Ok(Self(share))
        } else {
            Err(NotAShare(share.to_string()))
        }
    }

    pub fn of(realtime: usize, legs: usize) -> Option<Self> {
        (legs > 0).then(|| Self(realtime as f64 / legs as f64))
    }
}

impl FromStr for RealtimeShare {
    type Err = NotAShare;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let share = text.parse().map_err(|_| NotAShare(text.to_string()))?;
        Self::new(share)
    }
}

impl Display for RealtimeShare {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn a_share_reads_from_text() {
        assert_eq!("0.5".parse(), Ok(RealtimeShare(0.5)));
        assert_eq!("1".parse(), Ok(RealtimeShare(1.0)));
    }

    #[test]
    fn text_that_is_no_number_is_refused() {
        assert_eq!(
            "half".parse::<RealtimeShare>(),
            Err(NotAShare("half".to_string()))
        );
    }

    #[test]
    fn a_share_that_is_no_finite_number_is_refused() {
        for share in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(RealtimeShare::new(share).is_err(), "{share}");
        }
    }

    #[test]
    fn the_share_of_legs_is_the_realtime_count_over_all() {
        assert_eq!(RealtimeShare::of(3, 4), Some(RealtimeShare(0.75)));
        assert_eq!(RealtimeShare::of(0, 4), Some(RealtimeShare(0.0)));
        assert_eq!(RealtimeShare::of(4, 4), Some(RealtimeShare(1.0)));
    }

    #[test]
    fn no_legs_have_no_share() {
        assert_eq!(RealtimeShare::of(0, 0), None);
    }

    proptest! {
        #[test]
        fn a_share_below_0_is_refused(share in f64::MIN..0.0) {
            prop_assert!(RealtimeShare::new(share).is_err());
        }

        #[test]
        fn a_share_above_1_is_refused(share in 1.0f64.next_up()..f64::MAX) {
            prop_assert!(RealtimeShare::new(share).is_err());
        }

        #[test]
        fn a_share_survives_display_and_parse(share in 0.0..=1.0f64) {
            let realtime = RealtimeShare::new(share).expect("a share from 0 to 1");
            prop_assert_eq!(realtime.to_string().parse(), Ok(realtime));
        }
    }
}
