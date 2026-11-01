use std::collections::HashMap;

use geo_types::Point;
use medallion::{Countries, Country};

pub(crate) struct MemoisedCountries<'a, C> {
    countries: &'a C,
    by_point: HashMap<(u64, u64), Option<Country>>,
}

impl<'a, C: Countries> MemoisedCountries<'a, C> {
    pub(crate) fn of(countries: &'a C) -> Self {
        Self {
            countries,
            by_point: HashMap::new(),
        }
    }

    pub(crate) fn containing(&mut self, point: Point<f64>) -> Option<Country> {
        *self
            .by_point
            .entry((point.x().to_bits(), point.y().to_bits()))
            .or_insert_with(|| self.countries.containing(point))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use proptest::prelude::*;

    use super::*;

    #[derive(Default)]
    struct EastIsGermany {
        lookups: Cell<usize>,
    }

    impl Countries for EastIsGermany {
        fn containing(&self, point: Point<f64>) -> Option<Country> {
            self.lookups.set(self.lookups.get() + 1);
            (point.x() > 0.0).then_some(Country::Germany)
        }
    }

    #[test]
    fn a_point_asked_about_again_is_looked_up_once() {
        let countries = EastIsGermany::default();
        let mut memoised = MemoisedCountries::of(&countries);

        let answers = [(); 3].map(|_| memoised.containing(Point::new(8.0, 50.0)));

        assert_eq!(answers, [Some(Country::Germany); 3]);
        assert_eq!(countries.lookups.get(), 1);
    }

    #[test]
    fn a_point_in_no_country_is_looked_up_once() {
        let countries = EastIsGermany::default();
        let mut memoised = MemoisedCountries::of(&countries);

        let answers = [(); 3].map(|_| memoised.containing(Point::new(-8.0, 50.0)));

        assert_eq!(answers, [None; 3]);
        assert_eq!(countries.lookups.get(), 1);
    }

    #[test]
    fn each_distinct_point_is_looked_up() {
        let countries = EastIsGermany::default();
        let mut memoised = MemoisedCountries::of(&countries);

        memoised.containing(Point::new(8.0, 50.0));
        memoised.containing(Point::new(8.0, 50.1));
        memoised.containing(Point::new(8.1, 50.0));

        assert_eq!(countries.lookups.get(), 3);
    }

    proptest! {
        #[test]
        fn memoised_answers_are_the_answers_of_the_countries(
            points in prop::collection::vec((-3..3i8, -3..3i8), 0..50),
        ) {
            let countries = EastIsGermany::default();
            let mut memoised = MemoisedCountries::of(&countries);

            for (x, y) in points {
                let point = Point::new(f64::from(x), f64::from(y));
                prop_assert_eq!(memoised.containing(point), countries.containing(point));
            }
        }
    }
}
