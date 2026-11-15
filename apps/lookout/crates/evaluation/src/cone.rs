use std::ops::Add;

use chrono::{DateTime, TimeDelta, Utc};
use geo::{Bearing, Distance, Geodesic};
use geo_types::Point;
use itertools::Itertools;
use medallion_model::TrainSessionSamplePositionV3;

use crate::a5::NoA5Cell;
use crate::split::ScoredSplit;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConeSettings {
    pub horizon: TimeDelta,
    pub dwelling_metres: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub t: DateTime<Utc>,
    pub point: Point<f64>,
    pub in_split: bool,
    pub previous: Option<Point<f64>>,
    pub step_metres: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    pub start: usize,
    pub later: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub start: usize,
    pub travelled_metres: f64,
    pub wholly_in_split: bool,
    pub dwelling: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PairAngle {
    pub start: usize,
    pub later: usize,
    pub degrees: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowAngle {
    pub started_at: DateTime<Utc>,
    pub degrees: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConeAngles {
    pub samples: usize,
    pub repeats_position: usize,
    pub starts: usize,
    pub pairs: usize,
    pub windows: usize,
    pub in_split: usize,
    pub dwelling: usize,
    pub pair_angles: usize,
    pub pairs_without_bearing: usize,
    pub window_angles: Vec<WindowAngle>,
}

impl Step {
    pub fn repeats_position(&self) -> bool {
        self.previous == Some(self.point)
    }

    fn has_heading(&self) -> bool {
        self.previous.is_some() && !self.repeats_position()
    }
}

pub fn steps(
    samples: &[TrainSessionSamplePositionV3],
    split: &ScoredSplit,
) -> Result<Vec<Step>, NoA5Cell> {
    let points: Vec<Point<f64>> = samples
        .iter()
        .map(|sample| Point::new(sample.lon, sample.lat))
        .collect();
    let previous = std::iter::once(None).chain(points.iter().copied().map(Some));
    samples
        .iter()
        .zip(&points)
        .zip(previous)
        .map(|((sample, &point), previous)| {
            Ok(Step {
                t: sample.t,
                point,
                in_split: split.contains(point)?,
                previous,
                step_metres: previous.map(|previous| Geodesic.distance(previous, point)),
            })
        })
        .collect()
}

pub fn pairs(steps: &[Step], horizon: TimeDelta) -> Vec<Pair> {
    steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.has_heading())
        .flat_map(|(start, step)| {
            steps[start + 1..]
                .iter()
                .take_while(move |later| later.t <= step.t + horizon)
                .enumerate()
                .map(move |(offset, _)| Pair {
                    start,
                    later: start + 1 + offset,
                })
        })
        .collect()
}

pub fn windows(steps: &[Step], pairs: &[Pair], dwelling_metres: f64) -> Vec<Window> {
    pairs
        .iter()
        .chunk_by(|pair| pair.start)
        .into_iter()
        .map(|(start, later)| {
            let end = later.map(|pair| pair.later).max().unwrap_or(start);
            let travelled_metres = steps[start + 1..=end]
                .iter()
                .filter_map(|step| step.step_metres)
                .sum();
            Window {
                start,
                travelled_metres,
                wholly_in_split: steps[start - 1..=end].iter().all(|step| step.in_split),
                dwelling: travelled_metres < dwelling_metres,
            }
        })
        .collect()
}

pub fn pair_angles(steps: &[Step], pairs: &[Pair], kept: &[Window]) -> (Vec<PairAngle>, usize) {
    let kept: Vec<usize> = kept.iter().map(|window| window.start).collect();
    let (with_bearing, without_bearing): (Vec<&Pair>, Vec<&Pair>) = pairs
        .iter()
        .filter(|pair| kept.binary_search(&pair.start).is_ok())
        .partition(|pair| steps[pair.later].point != steps[pair.start].point);
    let angles = with_bearing
        .into_iter()
        .filter_map(|pair| {
            let start = &steps[pair.start];
            start.previous.map(|previous| PairAngle {
                start: pair.start,
                later: pair.later,
                degrees: angle_between(
                    Geodesic.bearing(previous, start.point),
                    Geodesic.bearing(start.point, steps[pair.later].point),
                ),
            })
        })
        .collect();
    (angles, without_bearing.len())
}

pub fn window_angles(steps: &[Step], pair_angles: &[PairAngle]) -> Vec<WindowAngle> {
    pair_angles
        .iter()
        .chunk_by(|pair| pair.start)
        .into_iter()
        .filter_map(|(start, angles)| {
            angles
                .map(|pair| pair.degrees)
                .reduce(f64::max)
                .map(|degrees| WindowAngle {
                    started_at: steps[start].t,
                    degrees,
                })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConeStages {
    pub steps: Vec<Step>,
    pub pairs: Vec<Pair>,
    pub windows: Vec<Window>,
    pub pair_angles: Vec<PairAngle>,
    pub pairs_without_bearing: usize,
    pub window_angles: Vec<WindowAngle>,
}

pub fn cone_stages(
    samples: &[TrainSessionSamplePositionV3],
    split: &ScoredSplit,
    settings: &ConeSettings,
) -> Result<ConeStages, NoA5Cell> {
    let steps = steps(samples, split)?;
    let pairs = pairs(&steps, settings.horizon);
    let windows = windows(&steps, &pairs, settings.dwelling_metres);
    let kept: Vec<Window> = windows
        .iter()
        .filter(|window| window.wholly_in_split && !window.dwelling)
        .copied()
        .collect();
    let (pair_angles, pairs_without_bearing) = pair_angles(&steps, &pairs, &kept);
    let window_angles = window_angles(&steps, &pair_angles);
    Ok(ConeStages {
        steps,
        pairs,
        windows,
        pair_angles,
        pairs_without_bearing,
        window_angles,
    })
}

pub fn cone_angles(
    samples: &[TrainSessionSamplePositionV3],
    split: &ScoredSplit,
    settings: &ConeSettings,
) -> Result<ConeAngles, NoA5Cell> {
    let stages = cone_stages(samples, split, settings)?;
    let windows = &stages.windows;
    Ok(ConeAngles {
        samples: stages.steps.len(),
        repeats_position: stages
            .steps
            .iter()
            .filter(|step| step.repeats_position())
            .count(),
        starts: stages
            .steps
            .iter()
            .filter(|step| step.has_heading())
            .count(),
        pairs: stages.pairs.len(),
        windows: windows.len(),
        in_split: windows
            .iter()
            .filter(|window| window.wholly_in_split)
            .count(),
        dwelling: windows
            .iter()
            .filter(|window| window.wholly_in_split && window.dwelling)
            .count(),
        pair_angles: stages.pair_angles.len(),
        pairs_without_bearing: stages.pairs_without_bearing,
        window_angles: stages.window_angles,
    })
}

fn angle_between(heading: f64, bearing: f64) -> f64 {
    ((bearing - heading + 180.0).rem_euclid(360.0) - 180.0).abs()
}

impl Add for ConeAngles {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            samples: self.samples + other.samples,
            repeats_position: self.repeats_position + other.repeats_position,
            starts: self.starts + other.starts,
            pairs: self.pairs + other.pairs,
            windows: self.windows + other.windows,
            in_split: self.in_split + other.in_split,
            dwelling: self.dwelling + other.dwelling,
            pair_angles: self.pair_angles + other.pair_angles,
            pairs_without_bearing: self.pairs_without_bearing + other.pairs_without_bearing,
            window_angles: self
                .window_angles
                .into_iter()
                .chain(other.window_angles)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use domain::SessionId;

    use super::*;
    use crate::fixtures::instant;
    use crate::split::{Split, Splitter, TuneBuckets};

    const TUNE_LON: f64 = 13.7322;
    const TUNE_LAT: f64 = 50.7403;
    const EAST_110_METRES: f64 = 0.00156;
    const NORTH_110_METRES: f64 = 0.000989;

    fn scored(split: Split) -> ScoredSplit {
        ScoredSplit {
            splitter: Splitter {
                resolution: "8".parse().unwrap(),
                salt: "tune-test-1".parse().unwrap(),
                tune_buckets: TuneBuckets::new(70).unwrap(),
            },
            split,
        }
    }

    fn settings() -> ConeSettings {
        ConeSettings {
            horizon: TimeDelta::minutes(5),
            dwelling_metres: 500.0,
        }
    }

    fn track(positions: &[(f64, f64)]) -> Vec<TrainSessionSamplePositionV3> {
        positions
            .iter()
            .zip(0..)
            .map(|(&(lon, lat), n)| TrainSessionSamplePositionV3 {
                session_id: SessionId::new("session-a").unwrap(),
                t: instant(i64::from(n) * 10),
                seq: n,
                lon,
                lat,
            })
            .collect()
    }

    fn east(steps: usize) -> Vec<(f64, f64)> {
        (0..steps)
            .map(|n| (TUNE_LON + EAST_110_METRES * n as f64, TUNE_LAT))
            .collect()
    }

    fn east_then_north(east_steps: usize, north_steps: usize) -> Vec<(f64, f64)> {
        let corner_lon = TUNE_LON + EAST_110_METRES * (east_steps - 1) as f64;
        east(east_steps)
            .into_iter()
            .chain((1..=north_steps).map(|n| (corner_lon, TUNE_LAT + NORTH_110_METRES * n as f64)))
            .collect()
    }

    #[test]
    fn a_straight_track_counts_its_windows_and_leaves_the_last_few_out_as_dwelling() {
        let angles = cone_angles(&track(&east(40)), &scored(Split::Tune), &settings()).unwrap();

        assert_eq!(
            (
                angles.repeats_position,
                angles.windows,
                angles.in_split,
                angles.dwelling,
                angles.window_angles.len()
            ),
            (0, 38, 38, 4, 34)
        );
    }

    #[test]
    fn a_straight_track_keeps_every_later_position_on_its_heading() {
        let angles = cone_angles(&track(&east(40)), &scored(Split::Tune), &settings()).unwrap();

        assert!(
            angles
                .window_angles
                .iter()
                .all(|window| window.degrees < 0.1),
            "{:?}",
            angles.window_angles
        );
    }

    #[test]
    fn a_train_turning_north_at_a_corner_needs_a_right_angle_from_the_corner() {
        let angles = cone_angles(
            &track(&east_then_north(10, 20)),
            &scored(Split::Tune),
            &settings(),
        )
        .unwrap();

        let at_the_corner = angles
            .window_angles
            .iter()
            .find(|window| window.started_at == instant(90))
            .expect("the window starting at the corner");
        assert!(
            (at_the_corner.degrees - 90.0).abs() < 0.1,
            "{at_the_corner:?}"
        );
    }

    #[test]
    fn a_sample_repeating_the_position_before_it_starts_no_window() {
        let mut positions = east(40);
        positions[5] = positions[4];

        let angles = cone_angles(&track(&positions), &scored(Split::Tune), &settings()).unwrap();

        assert_eq!((angles.repeats_position, angles.windows), (1, 37));
    }

    #[test]
    fn a_window_outside_the_split_is_counted_and_not_measured() {
        let angles = cone_angles(&track(&east(40)), &scored(Split::Test), &settings()).unwrap();

        assert_eq!(
            (angles.windows, angles.in_split, angles.window_angles.len()),
            (38, 0, 0)
        );
    }
}
