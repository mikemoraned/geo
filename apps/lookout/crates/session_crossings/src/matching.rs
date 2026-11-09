use std::cmp::Ordering;
use std::num::NonZeroU32;

use chrono::{DateTime, Utc};
use domain::{Pass, Radius, SessionId, TooManySamples};
use geo_types::Point;
use rstar::primitives::GeomWithData;
use rstar::{PointDistance, RTree};

pub const DEFAULT_RADIUS: Radius =
    Radius::metres(NonZeroU32::new(250).expect("two hundred and fifty is not zero"));

#[derive(Debug, Clone)]
pub struct Session {
    pub session_id: SessionId,
    pub samples: Vec<Sample>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub t: DateTime<Utc>,
    pub projected: Point<f64>,
}

impl Sample {
    pub fn session_order(&self, other: &Self) -> Ordering {
        self.t
            .cmp(&other.t)
            .then(self.projected.x().total_cmp(&other.projected.x()))
            .then(self.projected.y().total_cmp(&other.projected.y()))
    }
}

#[derive(Debug, Clone)]
pub struct Crossing {
    pub crossing: domain::Crossing,
    pub projected: Point<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NoPass {
    #[error("a visit to a crossing holds no sample")]
    EmptyVisit,
    #[error(transparent)]
    TooManySamples(#[from] TooManySamples),
}

#[derive(Debug)]
pub struct CrossingIndex {
    crossings: Vec<Crossing>,
    tree: RTree<GeomWithData<[f64; 2], usize>>,
}

impl CrossingIndex {
    pub fn new(crossings: Vec<Crossing>) -> Self {
        let tree = RTree::bulk_load(
            crossings
                .iter()
                .enumerate()
                .map(|(n, crossing)| GeomWithData::new(coordinates(crossing.projected), n))
                .collect(),
        );
        Self { crossings, tree }
    }

    pub fn crossings(&self) -> &[Crossing] {
        &self.crossings
    }

    fn near(&self, sample: &Sample, radius: Radius) -> impl Iterator<Item = (usize, f64)> + '_ {
        let at = coordinates(sample.projected);
        self.tree
            .locate_within_distance(at, squared(radius))
            .map(move |found| (found.data, found.distance_2(&at).sqrt()))
    }
}

#[derive(Debug, Clone)]
struct Hit {
    session_id: SessionId,
    crossing: usize,
    rank: u64,
    sample: Sample,
    distance_metres: f64,
}

#[derive(Debug)]
pub struct Matcher<'a> {
    index: &'a CrossingIndex,
    radius: Radius,
    hits: Vec<Hit>,
}

impl<'a> Matcher<'a> {
    pub fn new(index: &'a CrossingIndex, radius: Radius) -> Self {
        Self {
            index,
            radius,
            hits: Vec::new(),
        }
    }

    pub fn observe(&mut self, session_id: &SessionId, rank: u64, sample: &Sample) {
        let index = self.index;
        self.hits.extend(
            index
                .near(sample, self.radius)
                .map(|(crossing, distance_metres)| Hit {
                    session_id: session_id.clone(),
                    crossing,
                    rank,
                    sample: *sample,
                    distance_metres,
                }),
        );
    }

    pub fn passes(mut self) -> Result<Vec<Pass>, NoPass> {
        let index = self.index;
        let mut passed = runs(&mut self.hits)
            .map(|run| pass_of(index, run))
            .collect::<Result<Vec<_>, _>>()?;
        passed.sort_by(|a, b| (a.crossed_at, &a.crossing_id).cmp(&(b.crossed_at, &b.crossing_id)));
        Ok(passed)
    }

    pub fn visits(mut self) -> Result<Vec<Visit>, NoPass> {
        let index = self.index;
        runs(&mut self.hits)
            .map(|run| {
                Ok(Visit {
                    near: run
                        .iter()
                        .map(|hit| Near {
                            sample: hit.sample,
                            distance_metres: hit.distance_metres,
                        })
                        .collect(),
                    pass: pass_of(index, run)?,
                })
            })
            .collect()
    }
}

pub fn passes(
    sessions: &[Session],
    crossings: &[Crossing],
    radius: Radius,
) -> Result<Vec<Pass>, NoPass> {
    let index = CrossingIndex::new(crossings.to_vec());
    let mut matcher = Matcher::new(&index, radius);
    for session in sessions {
        for (rank, sample) in (0..).zip(&session.samples) {
            matcher.observe(&session.session_id, rank, sample);
        }
    }
    matcher.passes()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Near {
    pub sample: Sample,
    pub distance_metres: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Visit {
    pub near: Vec<Near>,
    pub pass: Pass,
}

pub fn visits(
    session_id: &SessionId,
    samples: &[Sample],
    crossing: &Crossing,
    radius: Radius,
) -> Result<Vec<Visit>, NoPass> {
    let index = CrossingIndex::new(vec![crossing.clone()]);
    let mut matcher = Matcher::new(&index, radius);
    for (rank, sample) in (0..).zip(samples) {
        matcher.observe(session_id, rank, sample);
    }
    matcher.visits()
}

fn runs(hits: &mut [Hit]) -> impl Iterator<Item = &[Hit]> {
    hits.sort_by(|a, b| {
        (&a.session_id, a.crossing, a.rank).cmp(&(&b.session_id, b.crossing, b.rank))
    });
    hits.chunk_by(|a, b| {
        a.session_id == b.session_id && a.crossing == b.crossing && b.rank == a.rank + 1
    })
}

fn pass_of(index: &CrossingIndex, run: &[Hit]) -> Result<Pass, NoPass> {
    let nearest = run
        .iter()
        .min_by(|a, b| a.distance_metres.total_cmp(&b.distance_metres))
        .ok_or(NoPass::EmptyVisit)?;

    Ok(Pass::new(
        nearest.session_id.clone(),
        index.crossings[nearest.crossing].crossing.id.clone(),
        nearest.sample.t,
        nearest.distance_metres,
        run.len(),
    )?)
}

fn coordinates(point: Point<f64>) -> [f64; 2] {
    [point.x(), point.y()]
}

fn squared(radius: Radius) -> f64 {
    radius.as_metres().powi(2)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use domain::places::BERLIN;
    use proptest::prelude::*;

    use super::*;

    const BERLIN_METRES: (f64, f64) = (798_809.63, 5_828_000.60);

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 22, 9, minute, 0).unwrap()
    }

    fn radius(metres: f64) -> Radius {
        Radius::new(metres).expect("a radius")
    }

    fn session(samples: Vec<Sample>) -> Session {
        Session {
            session_id: SessionId::new("session-a").unwrap(),
            samples,
        }
    }

    fn sample(minute: u32, east: f64) -> Sample {
        Sample {
            t: at(minute),
            projected: Point::new(BERLIN_METRES.0 + east, BERLIN_METRES.1),
        }
    }

    fn crossing(id: &str, east: f64) -> Crossing {
        let degrees = east / 111_320.0 / f64::cos(BERLIN.y().to_radians());
        Crossing {
            crossing: domain::Crossing::at(
                domain::CrossingId::new(id).expect("a name"),
                BERLIN.y(),
                BERLIN.x() + degrees,
            )
            .expect("on the globe"),
            projected: Point::new(BERLIN_METRES.0 + east, BERLIN_METRES.1),
        }
    }

    #[test]
    fn samples_in_a_session_are_ordered_by_time_and_then_by_place() {
        let mut samples = [
            sample(1, 20.0),
            Sample {
                projected: Point::new(BERLIN_METRES.0, BERLIN_METRES.1 + 5.0),
                ..sample(0, 0.0)
            },
            sample(1, 10.0),
            sample(0, 0.0),
        ];

        samples.sort_by(Sample::session_order);

        assert_eq!(
            samples
                .iter()
                .map(|sample| (
                    sample.t,
                    sample.projected.x() - BERLIN_METRES.0,
                    sample.projected.y() - BERLIN_METRES.1
                ))
                .collect::<Vec<_>>(),
            [
                (at(0), 0.0, 0.0),
                (at(0), 0.0, 5.0),
                (at(1), 10.0, 0.0),
                (at(1), 20.0, 0.0)
            ]
        );
    }

    #[test]
    fn a_crossing_the_session_came_within_the_radius_of_was_passed() {
        let session = session(vec![sample(0, 0.0), sample(1, 500.0)]);

        let passed = passes(&[session], &[crossing("c", 480.0)], radius(100.0)).expect("passes");

        assert_eq!(passed.len(), 1);
        assert_eq!(passed[0].crossing_id.to_string(), "c");
    }

    #[test]
    fn a_crossing_no_sample_came_near_was_not() {
        let session = session(vec![sample(0, 0.0), sample(1, 500.0)]);

        let passed = passes(&[session], &[crossing("c", 5_000.0)], radius(100.0)).expect("passes");

        assert!(passed.is_empty(), "{passed:?}");
    }

    #[test]
    fn the_nearest_sample_says_when_the_crossing_was_passed() {
        let session = session(vec![sample(0, 0.0), sample(1, 90.0), sample(2, 180.0)]);

        let passed = passes(&[session], &[crossing("c", 200.0)], radius(150.0)).expect("passes");

        assert_eq!(passed[0].crossed_at, at(2));
        assert!(
            (passed[0].distance_metres - 20.0).abs() < 0.001,
            "{passed:?}"
        );
    }

    #[test]
    fn every_sample_inside_the_radius_is_counted() {
        let session = session(vec![sample(0, 0.0), sample(1, 90.0), sample(2, 180.0)]);

        let passed = passes(&[session], &[crossing("c", 100.0)], radius(150.0)).expect("passes");

        assert_eq!(passed[0].samples_within, 3);
    }

    #[test]
    fn a_crossing_passed_by_many_samples_is_one_row() {
        let session = session((0..10).map(|i| sample(i, f64::from(i) * 10.0)).collect());

        let passed = passes(&[session], &[crossing("c", 50.0)], radius(150.0)).expect("passes");

        assert_eq!(passed.len(), 1);
    }

    #[test]
    fn a_crossing_at_exactly_the_radius_was_passed() {
        let session = session(vec![sample(0, 0.0)]);

        let passed = passes(&[session], &[crossing("c", 100.0)], radius(100.0)).expect("passes");

        assert_eq!(passed.len(), 1);
    }

    #[test]
    fn a_session_of_one_sample_passes_what_it_sat_next_to() {
        let session = session(vec![sample(0, 0.0)]);

        let passed = passes(&[session], &[crossing("c", 10.0)], radius(100.0)).expect("passes");

        assert_eq!(passed[0].samples_within, 1);
    }

    #[test]
    fn a_session_reversing_over_a_crossing_passes_it_once_per_visit() {
        let session = session(vec![
            sample(0, 0.0),
            sample(1, 200.0),
            sample(2, 400.0),
            sample(3, 200.0),
            sample(4, 0.0),
        ]);

        let passed = passes(&[session], &[crossing("c", 200.0)], radius(50.0)).expect("passes");

        assert_eq!(
            passed
                .iter()
                .map(|pass| pass.crossed_at)
                .collect::<Vec<_>>(),
            [at(1), at(3)]
        );
    }

    #[test]
    fn each_visit_counts_its_own_samples() {
        let session = session(vec![
            sample(0, 190.0),
            sample(1, 200.0),
            sample(2, 210.0),
            sample(3, 500.0),
            sample(4, 200.0),
        ]);

        let passed = passes(&[session], &[crossing("c", 200.0)], radius(50.0)).expect("passes");

        assert_eq!(
            passed
                .iter()
                .map(|pass| pass.samples_within)
                .collect::<Vec<_>>(),
            [3, 1]
        );
    }

    #[test]
    fn each_visit_holds_its_run_of_near_samples() {
        let session = session(vec![
            sample(0, 0.0),
            sample(1, 190.0),
            sample(2, 210.0),
            sample(3, 400.0),
            sample(4, 200.0),
        ]);

        let visits = visits(
            &session.session_id,
            &session.samples,
            &crossing("c", 200.0),
            radius(50.0),
        )
        .expect("visits");

        assert_eq!(
            visits
                .iter()
                .map(|visit| visit.near.iter().map(|near| near.sample.t).collect())
                .collect::<Vec<Vec<_>>>(),
            [vec![at(1), at(2)], vec![at(4)]]
        );
        assert!((visits[0].near[1].distance_metres - 10.0).abs() < 0.001);
    }

    #[test]
    fn the_visits_end_in_the_passes_the_matching_records() {
        let session = session(vec![
            sample(0, 0.0),
            sample(1, 200.0),
            sample(2, 400.0),
            sample(3, 200.0),
        ]);
        let crossing = crossing("c", 200.0);

        let visits = visits(
            &session.session_id,
            &session.samples,
            &crossing,
            radius(50.0),
        )
        .expect("visits");

        assert_eq!(visits.len(), 2);
        assert_eq!(
            visits
                .into_iter()
                .map(|visit| visit.pass)
                .collect::<Vec<_>>(),
            passes(&[session], &[crossing], radius(50.0)).expect("passes")
        );
    }

    #[test]
    fn rows_come_back_in_the_order_they_were_crossed() {
        let session = session(vec![sample(0, 0.0), sample(5, 1_000.0)]);

        let passed = passes(
            &[session],
            &[crossing("late", 1_000.0), crossing("early", 0.0)],
            radius(50.0),
        )
        .expect("passes");

        assert_eq!(
            passed
                .iter()
                .map(|pass| pass.crossing_id.to_string())
                .collect::<Vec<_>>(),
            ["early", "late"]
        );
    }

    fn placed(id: &str, x: f64, y: f64) -> Crossing {
        Crossing {
            crossing: domain::Crossing::at(
                domain::CrossingId::new(id).expect("a name"),
                BERLIN.y(),
                BERLIN.x(),
            )
            .expect("on the globe"),
            projected: Point::new(BERLIN_METRES.0 + x, BERLIN_METRES.1 + y),
        }
    }

    fn scanned(sessions: &[Session], crossings: &[Crossing], radius: Radius) -> Vec<Pass> {
        sessions
            .iter()
            .flat_map(|session| {
                crossings.iter().flat_map(|crossing| {
                    visits(&session.session_id, &session.samples, crossing, radius)
                        .expect("visits")
                        .into_iter()
                        .map(|visit| visit.pass)
                })
            })
            .collect()
    }

    fn ordered(mut passes: Vec<Pass>) -> Vec<Pass> {
        passes.sort_by(|a, b| {
            (&a.session_id, &a.crossing_id, a.crossed_at).cmp(&(
                &b.session_id,
                &b.crossing_id,
                b.crossed_at,
            ))
        });
        passes
    }

    proptest! {
        #[test]
        fn the_index_finds_the_passes_a_scan_of_every_sample_finds(
            walks in proptest::collection::vec(
                proptest::collection::vec((0.0..2_000.0f64, 0.0..2_000.0f64), 0..40),
                1..4,
            ),
            places in proptest::collection::vec((0.0..2_000.0f64, 0.0..2_000.0f64), 1..12),
            metres in 10.0..500.0f64,
        ) {
            let sessions: Vec<Session> = walks
                .iter()
                .enumerate()
                .map(|(n, walk)| Session {
                    session_id: SessionId::new(format!("session-{n}")).expect("an id"),
                    samples: (0..)
                        .zip(walk)
                        .map(|(minute, (x, y))| Sample {
                            t: at(minute),
                            projected: Point::new(BERLIN_METRES.0 + x, BERLIN_METRES.1 + y),
                        })
                        .collect(),
                })
                .collect();
            let crossings: Vec<Crossing> = places
                .iter()
                .enumerate()
                .map(|(n, (x, y))| placed(&format!("crossing-{n}"), *x, *y))
                .collect();

            let passed = passes(&sessions, &crossings, radius(metres)).expect("passes");

            prop_assert!(passed.iter().all(|pass| pass.distance_metres <= metres));
            prop_assert_eq!(
                ordered(passed),
                ordered(scanned(&sessions, &crossings, radius(metres)))
            );
        }
    }
}
