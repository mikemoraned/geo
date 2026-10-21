use std::fmt::{self, Display};
use std::str::FromStr;
use std::time::Duration;

use chrono::TimeDelta;

use crate::age::Age;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span(Duration);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotASpan {
    #[error("`{text}` is not a span of time, such as 30s or 4h: {reason}")]
    Unreadable { text: String, reason: String },
    #[error("`{text}` is no time at all; a span is longer than zero")]
    Zero { text: String },
    #[error("`{text}` is longer than a span can be")]
    TooLong { text: String },
}

impl Span {
    pub fn new(text: &str) -> Result<Self, NotASpan> {
        let duration = humantime::parse_duration(text).map_err(|err| NotASpan::Unreadable {
            text: text.to_string(),
            reason: err.to_string(),
        })?;
        if duration.is_zero() {
            return Err(NotASpan::Zero {
                text: text.to_string(),
            });
        }
        if TimeDelta::from_std(duration).is_err() {
            return Err(NotASpan::TooLong {
                text: text.to_string(),
            });
        }
        Ok(Self(duration))
    }

    pub fn duration(self) -> Duration {
        self.0
    }

    pub fn time_delta(self) -> TimeDelta {
        TimeDelta::from_std(self.0).expect("a span fits in a TimeDelta, as Span::new checked")
    }

    pub fn age(self) -> Age {
        Age::new(self.time_delta()).expect("a span is longer than zero, as Span::new checked")
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
        humantime::format_duration(self.0).fmt(f)
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
                matches!(text.parse::<Span>(), Err(NotASpan::Zero { .. })),
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
    fn a_span_becomes_an_age_of_the_same_length() {
        assert_eq!(span("30m").age().span(), TimeDelta::minutes(30));
    }
}
