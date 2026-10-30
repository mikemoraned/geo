use core::num::NonZeroU32;

use chrono::{DateTime, TimeDelta, TimeZone};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Age(TimeDelta);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("an age is longer than zero, and {0} is not")]
pub struct NotAnAge(TimeDelta);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{0} is more seconds than a u32 counts")]
pub struct TooManySeconds(TimeDelta);

impl Age {
    pub fn new(span: TimeDelta) -> Result<Self, NotAnAge> {
        if span > TimeDelta::zero() {
            Ok(Self(span))
        } else {
            Err(NotAnAge(span))
        }
    }

    pub const fn minutes(minutes: NonZeroU32) -> Self {
        Self(TimeDelta::minutes(minutes.get() as i64))
    }

    pub fn span(self) -> TimeDelta {
        self.0
    }

    pub fn seconds(self) -> Result<u32, TooManySeconds> {
        self.0
            .num_seconds()
            .try_into()
            .map_err(|_| TooManySeconds(self.0))
    }

    pub fn before<Tz: TimeZone>(self, now: DateTime<Tz>) -> DateTime<Tz> {
        now - self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minutes(n: u32) -> NonZeroU32 {
        NonZeroU32::new(n).expect("a positive count")
    }

    #[test]
    fn a_positive_span_is_an_age() {
        assert_eq!(
            Age::new(TimeDelta::seconds(1)).map(Age::span),
            Ok(TimeDelta::seconds(1))
        );
    }

    #[test]
    fn a_zero_or_negative_span_is_not_an_age() {
        for span in [
            TimeDelta::zero(),
            TimeDelta::seconds(-1),
            TimeDelta::minutes(-30),
        ] {
            assert_eq!(Age::new(span), Err(NotAnAge(span)), "{span}");
        }
    }

    #[test]
    fn an_age_in_minutes_spans_that_many_minutes() {
        assert_eq!(Age::minutes(minutes(30)).span(), TimeDelta::minutes(30));
    }

    #[test]
    fn an_age_before_an_instant_is_that_much_earlier() {
        let now = DateTime::from_timestamp(1_791_700_000, 0).expect("an instant");

        assert_eq!(
            Age::minutes(minutes(5)).before(now),
            now - TimeDelta::minutes(5)
        );
    }

    #[test]
    fn an_age_counts_its_whole_seconds() {
        assert_eq!(
            Age::new(TimeDelta::milliseconds(90_500)).map(Age::seconds),
            Ok(Ok(90))
        );
    }

    #[test]
    fn an_age_of_more_seconds_than_a_u32_counts_is_refused() {
        let span = TimeDelta::seconds(i64::from(u32::MAX) + 1);

        assert_eq!(
            Age::new(span).map(Age::seconds),
            Ok(Err(TooManySeconds(span)))
        );
    }
}
