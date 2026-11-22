use std::fmt::{self, Display};
use std::num::NonZeroU32;
use std::str::FromStr;
use std::time::Duration;

use chrono::TimeDelta;

use crate::age::{Age, NotAnAge};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span(Age);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotASpan {
    #[error("`{text}` is not a span of time, such as 30s or 4h: {reason}")]
    Unreadable { text: String, reason: String },
    #[error("`{text}` is not a span of time: {source}")]
    NotAnAge {
        text: String,
        #[source]
        source: NotAnAge,
    },
    #[error("`{text}` is longer than a span can be")]
    TooLong { text: String },
    #[error(transparent)]
    Age(#[from] NotAnAge),
}

impl Span {
    pub fn new(text: &str) -> Result<Self, NotASpan> {
        let duration = humantime::parse_duration(text).map_err(|err| NotASpan::Unreadable {
            text: text.to_string(),
            reason: err.to_string(),
        })?;
        let delta = TimeDelta::from_std(duration).map_err(|_| NotASpan::TooLong {
            text: text.to_string(),
        })?;
        let age = Age::new(delta).map_err(|source| NotASpan::NotAnAge {
            text: text.to_string(),
            source,
        })?;
        Ok(Self(age))
    }

    pub const fn minutes(minutes: NonZeroU32) -> Self {
        Self(Age::minutes(minutes))
    }

    pub const fn duration(self) -> Duration {
        self.0.duration()
    }

    pub const fn time_delta(self) -> TimeDelta {
        self.0.span()
    }

    pub const fn age(self) -> Age {
        self.0
    }
}

impl From<Age> for Span {
    fn from(age: Age) -> Self {
        Self(age)
    }
}

impl TryFrom<TimeDelta> for Span {
    type Error = NotASpan;

    fn try_from(delta: TimeDelta) -> Result<Self, Self::Error> {
        Ok(Self(Age::new(delta)?))
    }
}

impl FromStr for Span {
    type Err = NotASpan;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

impl Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        humantime::format_duration(self.duration()).fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str) -> Span {
        text.parse().expect("a span")
    }

    #[test]
    fn a_span_reads_the_suffixes_a_person_writes() {
        for (text, seconds) in [
            ("30s", 30),
            ("5m", 300),
            ("4h", 14_400),
            ("1h 30m", 5_400),
            ("1d", 86_400),
        ] {
            assert_eq!(
                span(text).duration(),
                Duration::from_secs(seconds),
                "{text}"
            );
        }
    }

    #[test]
    fn a_span_of_minutes_is_the_span_written_in_minutes() {
        assert_eq!(
            Span::minutes(NonZeroU32::new(5).expect("not zero")),
            span("5m")
        );
    }

    #[test]
    fn a_span_is_written_back_as_it_reads() {
        assert_eq!(span("4h").to_string(), "4h");
        assert_eq!(span("90m").to_string(), "1h 30m");
    }

    #[test]
    fn text_that_is_no_span_is_refused_with_the_text() {
        for text in ["", "soon", "30", "-5m", "4 hours later"] {
            let err = text.parse::<Span>().expect_err(text);
            assert!(err.to_string().contains(text), "{err}");
        }
    }

    #[test]
    fn a_span_of_zero_is_refused() {
        for text in ["0s", "0m", "0h"] {
            assert!(
                matches!(text.parse::<Span>(), Err(NotASpan::NotAnAge { .. })),
                "{text}"
            );
        }
    }

    #[test]
    fn a_span_too_long_for_a_time_delta_is_refused() {
        assert!(matches!(
            "500000000y".parse::<Span>(),
            Err(NotASpan::TooLong { .. })
        ));
    }

    #[test]
    fn a_positive_time_delta_is_a_span_of_the_same_length() {
        assert_eq!(
            Span::try_from(TimeDelta::milliseconds(1_500)),
            Ok(span("1500ms"))
        );
    }

    #[test]
    fn a_time_delta_of_zero_or_less_is_refused() {
        for delta in [TimeDelta::zero(), TimeDelta::seconds(-30)] {
            assert!(
                matches!(Span::try_from(delta), Err(NotASpan::Age(_))),
                "{delta}"
            );
        }
    }

    #[test]
    fn a_span_becomes_an_age_of_the_same_length() {
        assert_eq!(span("30m").age().span(), TimeDelta::minutes(30));
    }

    #[test]
    fn an_age_becomes_a_span_of_the_same_length() {
        assert_eq!(Span::from(span("30m").age()), span("30m"));
    }
}
