//! Finding the crossings nearest a fix.
//!
//! One brute-force pass over the whole set, per fix. At 5,749 points and a fix a second there
//! is nothing to gain from an index, and a linear scan over two flat columns is the shape the
//! hardware likes anyway.
//!
//! The pass answers both questions at once — the few nearest, for the screen, and everything
//! inside a radius, for the predictor — because they are the same distances, and computing
//! them twice would be the only expensive thing here.

use geo::{Distance, Haversine, Point};

use crate::pointset::{Point as Crossing, PointSet};

/// A crossing and how far away it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Near {
    pub crossing: Crossing,
    pub metres: f32,
}

/// What one scan found, each list ordered nearest first.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Nearby {
    /// At most as many as were asked for.
    pub nearest: Vec<Near>,
    /// Every crossing inside the radius, however many that is.
    pub within: Vec<Near>,
}

/// Scans every crossing against `from`, keeping the `nearest` closest and everything within
/// `radius_metres`.
pub fn nearby(points: &PointSet, from: Point<f32>, nearest: usize, radius_metres: f32) -> Nearby {
    let mut found = Nearby {
        nearest: Vec::with_capacity(nearest),
        within: Vec::new(),
    };

    for (index, (latitude, longitude)) in points.positions().enumerate() {
        let metres = Haversine.distance(from, Point::new(longitude, latitude));
        let near = Near {
            crossing: points
                .get(index)
                .expect("an index the columns just yielded"),
            metres,
        };

        // Kept in order as they arrive, rather than sorted afterwards, so the whole set is
        // never collected — at 5,749 points that would be 69KB of heap per fix.
        let closer_than_the_worst_kept = found
            .nearest
            .last()
            .is_none_or(|worst| metres < worst.metres);
        if found.nearest.len() < nearest || closer_than_the_worst_kept {
            let at = found.nearest.partition_point(|kept| kept.metres <= metres);
            found.nearest.insert(at, near);
            found.nearest.truncate(nearest);
        }

        if metres <= radius_metres {
            found.within.push(near);
        }
    }

    found
        .within
        .sort_by(|one, other| one.metres.total_cmp(&other.metres));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rounded to the metre from an independent haversine over the same mean-radius sphere,
    /// so this checks the formula rather than checking `geo` against itself. The tolerance
    /// covers `f32` and the exact radius a given implementation picks.
    const DEGREE_OF_LATITUDE_M: f32 = 111_195.0;
    const TOLERANCE_M: f32 = 10.0;

    fn at(latitude: f32, longitude: f32) -> Point<f32> {
        Point::new(longitude, latitude)
    }

    fn set(points: &[(f32, f32)]) -> Vec<u8> {
        let mut packed = Vec::new();
        packed.extend_from_slice(b"XING");
        packed.extend_from_slice(&1u32.to_le_bytes());
        packed.extend_from_slice(&(points.len() as u32).to_le_bytes());
        packed.extend(points.iter().flat_map(|(lat, _)| lat.to_le_bytes()));
        packed.extend(points.iter().flat_map(|(_, lon)| lon.to_le_bytes()));
        packed.extend((0..points.len()).flat_map(|id| (id as u32).to_le_bytes()));
        packed
    }

    /// Keeps the packed bytes alive and 4-byte aligned for the borrowed `PointSet`.
    fn aligned(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks(4)
            .map(|word| u32::from_le_bytes(word.try_into().expect("whole words")))
            .collect()
    }

    fn scan(points: &[(f32, f32)], from: Point<f32>, nearest: usize, radius: f32) -> Nearby {
        let words = aligned(&set(points));
        let bytes: &[u8] = bytemuck::cast_slice(&words);
        nearby(
            &PointSet::new(bytes).expect("a set built here"),
            from,
            nearest,
            radius,
        )
    }

    #[test]
    fn a_degree_of_latitude_is_a_hundred_and_eleven_kilometres() {
        let found = scan(&[(1.0, 0.0)], at(0.0, 0.0), 1, f32::INFINITY);

        let metres = found.nearest[0].metres;
        assert!(
            (metres - DEGREE_OF_LATITUDE_M).abs() < TOLERANCE_M,
            "{metres}m is not a degree of latitude",
        );
    }

    /// The same span of longitude shrinks away from the equator — the reason distances are
    /// great-circle rather than scaled degrees.
    #[test]
    fn a_degree_of_longitude_shrinks_towards_the_pole() {
        let equator = scan(&[(0.0, 1.0)], at(0.0, 0.0), 1, f32::INFINITY);
        let germany = scan(&[(51.5, 1.0)], at(51.5, 0.0), 1, f32::INFINITY);

        assert!((equator.nearest[0].metres - DEGREE_OF_LATITUDE_M).abs() < TOLERANCE_M);
        let expected = DEGREE_OF_LATITUDE_M * 51.5f32.to_radians().cos();
        assert!((germany.nearest[0].metres - expected).abs() < TOLERANCE_M);
    }

    #[test]
    fn a_crossing_you_are_standing_on_is_no_distance_away() {
        let found = scan(&[(51.5, 13.5)], at(51.5, 13.5), 1, f32::INFINITY);

        assert_eq!(found.nearest[0].metres, 0.0);
    }

    #[test]
    fn the_nearest_come_back_nearest_first() {
        let points = [(51.3, 0.0), (51.1, 0.0), (51.2, 0.0)];

        let found = scan(&points, at(51.0, 0.0), 3, f32::INFINITY);

        let ids: Vec<u32> = found.nearest.iter().map(|near| near.crossing.id).collect();
        assert_eq!(ids, vec![1, 2, 0]);
    }

    #[test]
    fn only_as_many_as_were_asked_for_come_back() {
        let points = [(51.3, 0.0), (51.1, 0.0), (51.2, 0.0)];

        let found = scan(&points, at(51.0, 0.0), 2, f32::INFINITY);

        assert_eq!(found.nearest.len(), 2);
        assert_eq!(found.nearest[0].crossing.id, 1);
    }

    /// Asking for more than there are is what happens on a screen with room for five when the
    /// buffer holds three, and is not an error.
    #[test]
    fn asking_for_more_than_there_are_returns_what_there_is() {
        let points = [(51.1, 0.0), (51.2, 0.0)];

        let found = scan(&points, at(51.0, 0.0), 5, f32::INFINITY);

        assert_eq!(found.nearest.len(), 2);
    }

    #[test]
    fn an_empty_set_finds_nothing() {
        let found = scan(&[], at(51.0, 0.0), 5, f32::INFINITY);

        assert_eq!(found, Nearby::default());
    }

    /// A radius of a kilometre over points a degree apart: the near one is inside it, the far
    /// ones are not.
    #[test]
    fn only_the_crossings_inside_the_radius_come_back() {
        let points = [(51.0, 0.0), (52.0, 0.0), (53.0, 0.0)];

        let found = scan(&points, at(51.0, 0.0), 3, 1_000.0);

        let ids: Vec<u32> = found.within.iter().map(|near| near.crossing.id).collect();
        assert_eq!(ids, vec![0]);
    }

    #[test]
    fn a_crossing_exactly_on_the_radius_is_inside_it() {
        let points = [(1.0, 0.0)];

        let found = scan(&points, at(0.0, 0.0), 1, DEGREE_OF_LATITUDE_M + TOLERANCE_M);

        assert_eq!(found.within.len(), 1);
    }

    #[test]
    fn nothing_inside_the_radius_is_an_empty_list_not_an_error() {
        let points = [(52.0, 0.0)];

        let found = scan(&points, at(51.0, 0.0), 1, 1_000.0);

        assert!(found.within.is_empty());
        assert_eq!(found.nearest.len(), 1);
    }

    /// The two answers are one pass, so a crossing that appears in both has to carry the same
    /// distance in both — computing them separately is exactly what this rules out.
    #[test]
    fn both_answers_come_from_the_same_distances() {
        let points = [(51.001, 0.0), (51.002, 0.0), (51.003, 0.0), (52.0, 0.0)];

        let found = scan(&points, at(51.0, 0.0), 2, 1_000.0);

        for near in &found.nearest {
            if let Some(same) = found
                .within
                .iter()
                .find(|within| within.crossing.id == near.crossing.id)
            {
                assert_eq!(same.metres, near.metres);
                assert_eq!(same.crossing, near.crossing);
            }
        }
        assert_eq!(found.within.len(), 3, "three are inside a kilometre");
        assert_eq!(found.nearest.len(), 2);
    }

    #[test]
    fn the_crossings_inside_the_radius_come_back_nearest_first() {
        let points = [(51.003, 0.0), (51.001, 0.0), (51.002, 0.0)];

        let found = scan(&points, at(51.0, 0.0), 1, 1_000.0);

        let metres: Vec<f32> = found.within.iter().map(|near| near.metres).collect();
        assert!(
            metres.windows(2).all(|pair| pair[0] <= pair[1]),
            "{metres:?} is not ordered",
        );
    }

    /// The whole set, at the size the device really carries, still has to come back ordered
    /// and complete.
    #[test]
    fn a_realistic_set_scans_completely() {
        let points: Vec<(f32, f32)> = (0..5_749)
            .map(|index| (47.0 + index as f32 * 0.001, 13.0))
            .collect();

        let found = scan(&points, at(47.0, 13.0), 5, 10_000.0);

        assert_eq!(found.nearest.len(), 5);
        assert_eq!(found.nearest[0].crossing.id, 0);
        assert!(found.within.len() > 5);
    }
}
