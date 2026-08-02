//! Made-up crossings, for trying the device out before the real ones are on it.
//!
//! Spike 5 needs a point set of the right *size* rather than the right contents: what it is
//! measuring is whether a brute-force scan of that many points is affordable, and made-up
//! points cost exactly what real ones do. They are also safe to commit, which a dataset
//! derived from someone's travels would not be.

use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::bbox::Bbox;
use crate::pointset::{PackedId, Point};

/// Scattered uniformly through `window`, reproducibly for a given `seed`.
///
/// Ids are drawn from the same stream as the positions, and are distinct — which is all a real
/// crossing's id promises the device, and all a scan of made-up points depends on.
pub fn points(count: usize, window: &Bbox, seed: u64) -> Vec<Point> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let (min, max) = (window.min(), window.max());
    let mut taken = std::collections::HashSet::with_capacity(count);

    (0..count)
        .map(|_| {
            let longitude = rng.random_range(min.x..=max.x);
            let latitude = rng.random_range(min.y..=max.y);
            let id = std::iter::repeat_with(|| rng.random::<u32>())
                .find(|id| taken.insert(*id))
                .expect("a u32 not yet drawn, of which there are far more than count");
            Point::new(
                PackedId::from_bits(id),
                geo_types::coord! { x: longitude, y: latitude },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    const SEED: u64 = 5749;

    fn germany() -> Bbox {
        "6.08,47.42,15.04,54.93".parse().expect("a valid window")
    }

    #[test]
    fn as_many_points_as_were_asked_for() {
        assert_eq!(points(5_749, &germany(), SEED).len(), 5_749);
        assert!(points(0, &germany(), SEED).is_empty());
    }

    /// The generated set is committed and read back by a device that was flashed separately,
    /// so "same seed, same points" is the property that keeps the two in step.
    #[test]
    fn the_same_seed_gives_the_same_points() {
        assert_eq!(points(100, &germany(), SEED), points(100, &germany(), SEED));
    }

    #[test]
    fn a_different_seed_gives_different_points() {
        assert_ne!(
            points(100, &germany(), SEED),
            points(100, &germany(), SEED + 1)
        );
    }

    #[test]
    fn every_point_lands_inside_the_window() {
        let window = germany();

        for point in points(1_000, &window, SEED) {
            assert!(
                window.contains(point.longitude.into(), point.latitude.into()),
                "{point} is outside {window}",
            );
        }
    }

    /// Not a guarantee of the derivation — four bytes collide by chance — but at this size a
    /// clash would mean the made-up keys are not varying, which is worth knowing.
    #[test]
    fn the_points_have_distinct_ids() {
        let points = points(5_749, &germany(), SEED);

        let ids: HashSet<_> = points.iter().map(|point| point.id).collect();

        assert_eq!(ids.len(), points.len());
    }

    /// The set is meant to stand in for the real crossings, so it has to be spread over the
    /// window rather than bunched in a corner of it.
    #[test]
    fn the_points_are_spread_across_the_window() {
        let window = germany();
        let points = points(1_000, &window, SEED);

        let north = points
            .iter()
            .filter(|point| f64::from(point.latitude) > window.min().y.midpoint(window.max().y))
            .count();

        assert!((400..600).contains(&north), "{north} of 1000 are north");
    }
}
