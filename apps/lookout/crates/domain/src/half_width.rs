use std::fmt::{self, Display};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct HalfWidth(f64);

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum NotAHalfWidth {
    #[error("`{0}` is not a number of degrees, such as 45")]
    Unreadable(String),
    #[error("a half-width is more than 0 and at most 180 degrees, and {0} is not")]
    OutOfRange(f64),
}

impl HalfWidth {
    pub fn new(degrees: f64) -> Result<Self, NotAHalfWidth> {
        Self::checked(degrees).ok_or(NotAHalfWidth::OutOfRange(degrees))
    }

    pub const fn checked(degrees: f64) -> Option<Self> {
        if degrees > 0.0 && degrees <= 180.0 {
            Some(Self(degrees))
        } else {
            None
        }
    }

    pub const fn as_degrees(self) -> f64 {
        self.0
    }
}

impl FromStr for HalfWidth {
    type Err = NotAHalfWidth;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let degrees = text
            .parse()
            .map_err(|_| NotAHalfWidth::Unreadable(text.to_string()))?;
        Self::new(degrees)
    }
}

impl Display for HalfWidth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn a_number_of_degrees_above_0_and_up_to_180_is_a_half_width() {
        for degrees in [0.5, 30.0, 90.0, 180.0] {
            assert_eq!(
                HalfWidth::new(degrees).map(HalfWidth::as_degrees),
                Ok(degrees),
                "{degrees}"
            );
        }
    }

    #[test]
    fn a_number_of_degrees_outside_0_to_180_is_not_a_half_width() {
        for degrees in [0.0, -0.0, -30.0, 180.5, 360.0, f64::INFINITY, f64::NAN] {
            assert!(
                matches!(HalfWidth::new(degrees), Err(NotAHalfWidth::OutOfRange(_))),
                "{degrees}"
            );
        }
    }

    #[test]
    fn text_that_is_no_number_is_refused_with_the_text() {
        for text in ["", "wide", "45°", "4 5"] {
            assert_eq!(
                text.parse::<HalfWidth>(),
                Err(NotAHalfWidth::Unreadable(text.to_string())),
                "{text}"
            );
        }
    }

    #[test]
    fn a_number_read_that_is_not_a_half_width_is_refused() {
        for text in ["0", "-45", "181", "NaN"] {
            assert!(
                matches!(text.parse::<HalfWidth>(), Err(NotAHalfWidth::OutOfRange(_))),
                "{text}"
            );
        }
    }

    proptest! {
        #[test]
        fn a_half_width_is_written_back_as_it_reads(degrees in 0.0f64..=180.0) {
            prop_assume!(degrees > 0.0);
            let half_width = HalfWidth::new(degrees).expect("a number of degrees in range");

            prop_assert_eq!(half_width.to_string().parse::<HalfWidth>(), Ok(half_width));
        }
    }
}
