use std::collections::BTreeSet;

use chrono::TimeDelta;
use crossings::silver::Crossing;
use domain::CrossingCompactId;
use geo_types::Point;
use itertools::Itertools;
use medallion_model::TrainSessionSamplePositionV3;

use crate::a5::NoA5Cell;
use crate::split::ScoredSplit;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoredCrossings {
    Every,
    Within(BTreeSet<CrossingCompactId>),
}

impl ScoredCrossings {
    pub fn of(crossings: &[Crossing], split: Option<&ScoredSplit>) -> Result<Self, NoA5Cell> {
        let Some(split) = split else {
            return Ok(Self::Every);
        };
        crossings
            .iter()
            .map(|crossing| {
                split
                    .contains(crossing.crossing.position)
                    .map(|contained| contained.then_some(crossing.compact_id))
            })
            .filter_map(Result::transpose)
            .collect::<Result<_, _>>()
            .map(Self::Within)
    }

    pub fn contains(&self, crossing_compact_id: CrossingCompactId) -> bool {
        match self {
            Self::Every => true,
            Self::Within(within) => within.contains(&crossing_compact_id),
        }
    }

    pub fn keep<T>(
        &self,
        units: impl IntoIterator<Item = T>,
        crossing_of: impl Fn(&T) -> CrossingCompactId,
    ) -> Vec<T> {
        units
            .into_iter()
            .filter(|unit| self.contains(crossing_of(unit)))
            .collect()
    }
}

pub fn scored_hours(
    samples: &[TrainSessionSamplePositionV3],
    split: &ScoredSplit,
) -> Result<f64, NoA5Cell> {
    let scored = samples
        .iter()
        .tuple_windows()
        .map(|(first, next)| {
            split
                .contains(Point::new(first.lon, first.lat))
                .map(|contained| {
                    if contained {
                        next.t - first.t
                    } else {
                        TimeDelta::zero()
                    }
                })
        })
        .sum::<Result<TimeDelta, NoA5Cell>>()?;
    Ok(scored.as_seconds_f64() / 3_600.0)
}

#[cfg(test)]
mod tests {
    use domain::SessionId;
    use proptest::prelude::*;

    use super::*;
    use crate::fixtures::{crossing, instant};
    use crate::split::{Split, Splitter, TuneBuckets};

    fn splitter() -> Splitter {
        Splitter {
            resolution: "8".parse().unwrap(),
            salt: "tune-test-1".parse().unwrap(),
            tune_buckets: TuneBuckets::new(70).unwrap(),
        }
    }

    fn scored(split: Split) -> ScoredSplit {
        ScoredSplit {
            splitter: splitter(),
            split,
        }
    }

    fn germany() -> impl Strategy<Value = Point<f64>> {
        (5.9..15.1, 47.3..55.1).prop_map(|(lon, lat)| Point::new(lon, lat))
    }

    fn crossings(positions: &[Point<f64>]) -> Vec<Crossing> {
        positions
            .iter()
            .zip(0..)
            .map(|(&position, n)| Crossing {
                crossing: domain::Crossing::new(crossing(n), position),
                compact_id: CrossingCompactId::new(n),
                extract_id: "extract".to_string(),
            })
            .collect()
    }

    fn samples(steps: &[(Point<f64>, u16)]) -> Vec<TrainSessionSamplePositionV3> {
        steps
            .iter()
            .scan(0, |seconds, &(position, step)| {
                let t = instant(*seconds);
                *seconds += i64::from(step);
                Some((t, position))
            })
            .zip(0..)
            .map(|((t, position), seq)| TrainSessionSamplePositionV3 {
                session_id: SessionId::new("session-a").unwrap(),
                t,
                seq,
                lat: position.y(),
                lon: position.x(),
            })
            .collect()
    }

    fn hours_between(samples: &[TrainSessionSamplePositionV3]) -> f64 {
        match (samples.first(), samples.last()) {
            (Some(first), Some(last)) => (last.t - first.t).as_seconds_f64() / 3_600.0,
            _ => 0.0,
        }
    }

    #[test]
    fn a_run_without_a_split_scores_every_crossing() {
        let crossings = crossings(&[Point::new(13.7322, 51.0403)]);

        let scored = ScoredCrossings::of(&crossings, None).unwrap();

        assert_eq!(scored, ScoredCrossings::Every);
        assert_eq!(
            scored.keep([1, 2, 3], |&n| CrossingCompactId::new(n)),
            [1, 2, 3]
        );
    }

    #[test]
    fn dresden_hbf_is_scored_in_the_test_split_alone() {
        let crossings = crossings(&[Point::new(13.7322, 51.0403)]);

        let test = ScoredCrossings::of(&crossings, Some(&scored(Split::Test))).unwrap();
        let tune = ScoredCrossings::of(&crossings, Some(&scored(Split::Tune))).unwrap();

        assert!(test.contains(CrossingCompactId::new(0)));
        assert!(!tune.contains(CrossingCompactId::new(0)));
    }

    #[test]
    fn each_step_counts_towards_the_split_of_its_first_samples_cell() {
        let dresden = Point::new(13.7322, 51.0403);
        let samples = samples(&[(dresden, 600), (dresden, 1_200), (dresden, 0)]);

        assert_eq!(scored_hours(&samples, &scored(Split::Test)), Ok(0.5));
        assert_eq!(scored_hours(&samples, &scored(Split::Tune)), Ok(0.0));
    }

    proptest! {
        #[test]
        fn every_unit_falls_in_exactly_one_split(
            positions in prop::collection::vec(germany(), 1..50),
            units in prop::collection::vec(any::<prop::sample::Index>(), 0..200),
        ) {
            let crossings = crossings(&positions);
            let units: Vec<CrossingCompactId> = units
                .iter()
                .map(|unit| crossings[unit.index(crossings.len())].compact_id)
                .collect();
            let kept = |split| {
                ScoredCrossings::of(&crossings, Some(&scored(split)))
                    .unwrap()
                    .keep(units.clone(), |&unit| unit)
            };
            let all = ScoredCrossings::of(&crossings, None).unwrap().keep(units.clone(), |&unit| unit);
            let (tune, test) = (kept(Split::Tune), kept(Split::Test));

            prop_assert_eq!(tune.len() + test.len(), all.len());
            for unit in &units {
                prop_assert!(tune.contains(unit) != test.contains(unit));
            }
        }

        #[test]
        fn every_unit_takes_the_split_of_its_crossings_cell(
            positions in prop::collection::vec(germany(), 1..50),
            split in prop_oneof![Just(Split::Tune), Just(Split::Test)],
        ) {
            let crossings = crossings(&positions);
            let scored_crossings = ScoredCrossings::of(&crossings, Some(&scored(split))).unwrap();

            for crossing in &crossings {
                let cell = splitter().cell(crossing.crossing.position).unwrap();
                prop_assert_eq!(
                    scored_crossings.contains(crossing.compact_id),
                    splitter().split(cell) == split
                );
            }
        }

        #[test]
        fn the_scored_hours_of_both_splits_sum_to_the_sessions_hours(
            steps in prop::collection::vec((germany(), 1..3_600u16), 0..50),
        ) {
            let samples = samples(&steps);
            let tune = scored_hours(&samples, &scored(Split::Tune)).unwrap();
            let test = scored_hours(&samples, &scored(Split::Test)).unwrap();

            prop_assert!((tune + test - hours_between(&samples)).abs() < 1e-9);
            prop_assert!(tune >= 0.0 && test >= 0.0);
        }
    }
}
