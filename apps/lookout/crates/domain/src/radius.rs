use std::fmt::{self, Display};
use std::num::NonZeroU32;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Radius(f64);

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum NotARadius {
    #[error("`{0}` is not a number of metres, such as 250")]
    Unreadable(String),
    #[error("a radius is a positive, finite number of metres, and {0} is not")]
    OutOfRange(f64),
}

impl Radius {
    pub fn new(metres: f64) -> Result<Self, NotARadius> {
        if metres > 0.0 && metres.is_finite() {
            Ok(Self(metres))
        } else {
            Err(NotARadius::OutOfRange(metres))
        }
    }

    pub const fn metres(metres: NonZeroU32) -> Self {
        Self(metres.get() as f64)
    }

    pub const fn as_metres(self) -> f64 {
        self.0
    }
}

impl FromStr for Radius {
    type Err = NotARadius;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let metres = text
            .parse()
            .map_err(|_| NotARadius::Unreadable(text.to_string()))?;
        Self::new(metres)
    }
}

impl Display for Radius {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn a_positive_finite_number_of_metres_is_a_radius() {
        for metres in [0.5, 250.0, 5_000.0] {
            assert_eq!(
                Radius::new(metres).map(Radius::as_metres),
                Ok(metres),
                "{metres}"
            );
        }
    }

    #[test]
    fn a_number_that_is_not_positive_and_finite_is_not_a_radius() {
        for metres in [
            0.0,
            -0.0,
            -250.0,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
        ] {
            assert!(
                matches!(Radius::new(metres), Err(NotARadius::OutOfRange(_))),
                "{metres}"
            );
        }
    }

    #[test]
    fn a_radius_in_whole_metres_is_that_many_metres() {
        let metres = NonZeroU32::new(250).expect("a positive count");

        assert_eq!(Radius::metres(metres).as_metres(), 250.0);
    }

    #[test]
    fn text_that_is_no_number_is_refused_with_the_text() {
        for text in ["", "far", "250m", "2 50"] {
            assert_eq!(
                text.parse::<Radius>(),
                Err(NotARadius::Unreadable(text.to_string())),
                "{text}"
            );
        }
    }

    #[test]
    fn a_number_read_that_is_not_a_radius_is_refused() {
        for text in ["0", "-250", "inf", "NaN"] {
            assert!(
                matches!(text.parse::<Radius>(), Err(NotARadius::OutOfRange(_))),
                "{text}"
            );
        }
    }

    proptest! {
        #[test]
        fn a_radius_is_written_back_as_it_reads(
            metres in prop::num::f64::POSITIVE | prop::num::f64::NORMAL | prop::num::f64::SUBNORMAL
        ) {
            let radius = Radius::new(metres).expect("a positive, finite number is a radius");

            prop_assert_eq!(radius.to_string().parse::<Radius>(), Ok(radius));
        }
    }
}
