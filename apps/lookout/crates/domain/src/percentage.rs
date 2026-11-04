use hashfn::hashfn;
use nutype::nutype;

#[nutype(
    validate(with = percentage_in_range, error = NotAPercentage),
    derive(
        Debug, Clone, Copy, PartialEq, PartialOrd, Display, FromStr, TryFrom, Into, Serialize,
        Deserialize
    )
)]
pub struct Percentage(f64);

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[error("{0} is not a percentage, from 0 to 100")]
pub struct NotAPercentage(pub f64);

#[hashfn]
pub fn percentage_in_range(value: &f64) -> Result<(), NotAPercentage> {
    if (0.0..=100.0).contains(value) {
        Ok(())
    } else {
        Err(NotAPercentage(*value))
    }
}

impl Percentage {
    pub fn new(value: f64) -> Result<Self, NotAPercentage> {
        Self::try_new(value)
    }

    pub fn of(part: usize, whole: usize) -> Result<Self, NotAPercentage> {
        Self::new(100.0 * part as f64 / whole as f64)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn every_value_from_0_to_100_is_a_percentage(value in 0.0..=100.0f64) {
            prop_assert_eq!(Percentage::new(value).map(f64::from), Ok(value));
        }

        #[test]
        fn a_value_above_100_is_not_a_percentage(value in 100.000_001..f64::MAX) {
            prop_assert_eq!(Percentage::new(value), Err(NotAPercentage(value)));
        }

        #[test]
        fn a_value_below_0_is_not_a_percentage(value in f64::MIN..-0.000_001) {
            prop_assert_eq!(Percentage::new(value), Err(NotAPercentage(value)));
        }

        #[test]
        fn a_part_of_its_whole_is_a_percentage(whole in 1..10_000usize, share in 0.0..=1.0f64) {
            let part = (whole as f64 * share) as usize;

            let percentage = f64::from(Percentage::of(part, whole).expect("a percentage"));

            prop_assert!((0.0..=100.0).contains(&percentage), "{percentage}");
        }
    }

    #[test]
    fn not_a_number_is_not_a_percentage() {
        assert!(Percentage::new(f64::NAN).is_err());
    }

    #[test]
    fn a_quarter_of_the_whole_is_25_percent() {
        assert_eq!(Percentage::of(1, 4).map(f64::from), Ok(25.0));
    }

    #[test]
    fn a_part_of_nothing_is_not_a_percentage() {
        assert!(Percentage::of(0, 0).is_err());
    }

    #[test]
    fn more_than_the_whole_is_not_a_percentage() {
        assert!(Percentage::of(5, 4).is_err());
    }

    #[test]
    fn a_percentage_is_read_from_its_text() {
        assert_eq!("12.5".parse::<Percentage>().map(f64::from).ok(), Some(12.5));
    }
}
