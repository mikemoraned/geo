use std::str::FromStr;

use nutype::nutype;

#[nutype(
    validate(with = from_zero_upwards, error = NotAZoom),
    default = 8.0,
    derive(Debug, Clone, Copy, PartialEq, PartialOrd, Display, Default)
)]
pub struct Zoom(f64);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a zoom level, a number from 0 upwards")]
pub struct NotAZoom(String);

fn from_zero_upwards(level: &f64) -> Result<(), NotAZoom> {
    if level.is_finite() && *level >= 0.0 {
        Ok(())
    } else {
        Err(NotAZoom(level.to_string()))
    }
}

impl Zoom {
    pub fn new(level: f64) -> Result<Self, NotAZoom> {
        Self::try_new(level)
    }

    pub fn level(self) -> f64 {
        self.into_inner()
    }
}

impl FromStr for Zoom {
    type Err = NotAZoom;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let level = text.parse().map_err(|_| NotAZoom(text.to_string()))?;
        Self::new(level)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn a_level_reads_from_text() {
        assert_eq!("8".parse(), Zoom::new(8.0));
        assert_eq!("10.5".parse(), Zoom::new(10.5));
    }

    #[test]
    fn text_that_is_no_number_is_refused() {
        assert_eq!("close".parse::<Zoom>(), Err(NotAZoom("close".to_string())));
    }

    #[test]
    fn a_level_that_is_no_finite_number_is_refused() {
        for level in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(Zoom::new(level).is_err(), "{level}");
        }
    }

    proptest! {
        #[test]
        fn a_negative_level_is_refused(level in f64::MIN..0.0) {
            prop_assert!(Zoom::new(level).is_err());
        }

        #[test]
        fn a_level_survives_display_and_parse(level in 0.0..f64::MAX) {
            let zoom = Zoom::new(level).expect("a level from 0 upwards");
            prop_assert_eq!(zoom.to_string().parse(), Ok(zoom));
        }
    }
}
