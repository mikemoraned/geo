use std::collections::BTreeMap;
use std::iter::Sum;
use std::ops::Add;

use chrono::{DateTime, Utc};
use domain::{CrossingCompactId, Precision, Sample, SessionId};
use medallion_model::EvaluationSessionRowV1;
use predictor::{DetectPasses, Event, OutOfOrder};

use crate::offline::{PassesBySession, SessionPasses};
use crate::summarise::{Bootstrap, Estimate, Spread, estimate, resampled, spread};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Visit {
    pub first: DateTime<Utc>,
    pub last: DateTime<Utc>,
    pub crossing_compact_id: CrossingCompactId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Skew {
    pub sessions: usize,
    pub passes: usize,
    pub visits: usize,
    pub seen_passes: usize,
    pub unmatched_visits: usize,
    pub seen: Option<Estimate>,
    pub unmatched: Option<Estimate>,
    pub offsets: Option<Spread>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkewError {
    #[error("the session {session_id} was picked, and the offline passes hold nothing for it")]
    NoPasses { session_id: SessionId },
    #[error("the session {session_id} was picked, and the online visits hold nothing for it")]
    NoVisits { session_id: SessionId },
}

pub fn visits<F: Precision, D: DetectPasses<F>>(
    samples: impl IntoIterator<Item = Sample<F>>,
    mut detector: D,
) -> Result<Vec<Visit>, OutOfOrder> {
    let mut open: BTreeMap<CrossingCompactId, Visit> = BTreeMap::new();
    let mut ended = Vec::new();
    for sample in samples {
        detector.observe(Event::Sampled(sample))?;
        let passing = detector.passing();
        let (staying, left): (BTreeMap<_, _>, BTreeMap<_, _>) = open
            .into_iter()
            .partition(|(crossing_compact_id, _)| passing.contains(crossing_compact_id));
        ended.extend(left.into_values());
        open = passing
            .iter()
            .map(|&crossing_compact_id| {
                let first = staying
                    .get(&crossing_compact_id)
                    .map_or(sample.t, |visit| visit.first);
                (
                    crossing_compact_id,
                    Visit {
                        first,
                        last: sample.t,
                        crossing_compact_id,
                    },
                )
            })
            .collect();
    }
    ended.extend(open.into_values());
    ended.sort_unstable();
    Ok(ended)
}

pub fn skew(
    sessions: &[EvaluationSessionRowV1],
    passes: &PassesBySession,
    visits: &BTreeMap<SessionId, Vec<Visit>>,
    bootstrap: &Bootstrap,
) -> Result<Skew, SkewError> {
    let matches: Vec<(Tally, Vec<f64>)> = sessions
        .iter()
        .map(|session| {
            let session_id = &session.session_id;
            let passes = passes.get(session_id).ok_or_else(|| SkewError::NoPasses {
                session_id: session_id.clone(),
            })?;
            let visits = visits.get(session_id).ok_or_else(|| SkewError::NoVisits {
                session_id: session_id.clone(),
            })?;
            Ok(matched(passes, visits))
        })
        .collect::<Result<_, SkewError>>()?;
    let tallies: Vec<Tally> = matches.iter().map(|(tally, _)| *tally).collect();
    let whole: Tally = tallies.iter().copied().sum();
    let resamples = resampled(&tallies, bootstrap);
    Ok(Skew {
        sessions: sessions.len(),
        passes: whole.passes,
        visits: whole.visits,
        seen_passes: whole.seen_passes,
        unmatched_visits: whole.unmatched_visits,
        seen: estimate(&whole, &resamples, Tally::seen),
        unmatched: estimate(&whole, &resamples, Tally::unmatched),
        offsets: spread(
            matches
                .into_iter()
                .flat_map(|(_, offsets)| offsets)
                .collect(),
        ),
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Tally {
    passes: usize,
    seen_passes: usize,
    visits: usize,
    unmatched_visits: usize,
}

impl Tally {
    fn seen(&self) -> Option<f64> {
        (self.passes > 0).then(|| self.seen_passes as f64 / self.passes as f64)
    }

    fn unmatched(&self) -> Option<f64> {
        (self.visits > 0).then(|| self.unmatched_visits as f64 / self.visits as f64)
    }
}

impl Add for Tally {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            passes: self.passes + other.passes,
            seen_passes: self.seen_passes + other.seen_passes,
            visits: self.visits + other.visits,
            unmatched_visits: self.unmatched_visits + other.unmatched_visits,
        }
    }
}

impl Sum for Tally {
    fn sum<I: Iterator<Item = Self>>(tallies: I) -> Self {
        tallies.fold(Self::default(), Add::add)
    }
}

fn matched(passes: &SessionPasses, visits: &[Visit]) -> (Tally, Vec<f64>) {
    let covers =
        |visit: &Visit, &(crossed_at, crossing_compact_id): &(DateTime<Utc>, CrossingCompactId)| {
            visit.crossing_compact_id == crossing_compact_id
                && visit.first <= crossed_at
                && crossed_at <= visit.last
        };
    let offsets: Vec<f64> = passes
        .iter()
        .filter_map(|pass| {
            visits
                .iter()
                .find(|visit| covers(visit, pass))
                .map(|visit| (visit.first - pass.0).as_seconds_f64())
        })
        .collect();
    let unmatched_visits = visits
        .iter()
        .filter(|visit| !passes.iter().any(|pass| covers(visit, pass)))
        .count();
    (
        Tally {
            passes: passes.len(),
            seen_passes: offsets.len(),
            visits: visits.len(),
            unmatched_visits,
        },
        offsets,
    )
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use chrono::TimeDelta;
    use domain::places::DRESDEN_HBF;
    use medallion_model::TrainSessionRowV3;
    use predictor::{Event, Passes};

    use super::*;
    use crate::fixtures::instant;
    use crate::pick::Seed;
    use crate::record::session_rows;

    struct Scripted {
        rounds: Vec<Vec<u32>>,
        observed: usize,
        passing: Passes,
    }

    impl Scripted {
        fn new(rounds: &[&[u32]]) -> Self {
            Self {
                rounds: rounds.iter().map(|round| round.to_vec()).collect(),
                observed: 0,
                passing: Passes::new(),
            }
        }
    }

    impl DetectPasses<f64> for Scripted {
        fn observe(&mut self, _event: Event<f64>) -> Result<(), OutOfOrder> {
            self.passing = self.rounds[self.observed]
                .iter()
                .map(|&id| CrossingCompactId::new(id))
                .collect();
            self.observed += 1;
            Ok(())
        }

        fn passing(&self) -> &Passes {
            &self.passing
        }
    }

    fn samples(count: i64) -> Vec<Sample<f64>> {
        (0..count)
            .map(|step| {
                Sample::at(instant(step * 10), DRESDEN_HBF.y(), DRESDEN_HBF.x())
                    .expect("on the globe")
            })
            .collect()
    }

    fn visit(crossing: u32, first: i64, last: i64) -> Visit {
        Visit {
            first: instant(first),
            last: instant(last),
            crossing_compact_id: CrossingCompactId::new(crossing),
        }
    }

    fn pass(crossing: u32, at: i64) -> (DateTime<Utc>, CrossingCompactId) {
        (instant(at), CrossingCompactId::new(crossing))
    }

    fn session(name: &str) -> SessionId {
        SessionId::new(format!("session-{name}")).expect("a session id")
    }

    fn sessions(names: &[&str]) -> Vec<EvaluationSessionRowV1> {
        session_rows(
            &names
                .iter()
                .map(|name| TrainSessionRowV3 {
                    session_id: session(name),
                    started_at: instant(0),
                    ended_at: instant(3_600),
                    ..medallion::Row::example()
                })
                .collect::<Vec<_>>(),
        )
    }

    fn bootstrap() -> Bootstrap {
        Bootstrap {
            resamples: NonZeroUsize::new(200).expect("not zero"),
            seed: Seed(1),
        }
    }

    #[test]
    fn rounds_that_report_a_crossing_in_a_row_are_one_visit() {
        let visits = visits(samples(4), Scripted::new(&[&[], &[1], &[1], &[]])).expect("in order");

        assert_eq!(visits, [visit(1, 10, 20)]);
    }

    #[test]
    fn a_crossing_reported_again_after_a_gap_is_a_second_visit() {
        let visits = visits(samples(4), Scripted::new(&[&[1], &[], &[1], &[]])).expect("in order");

        assert_eq!(visits, [visit(1, 0, 0), visit(1, 20, 20)]);
    }

    #[test]
    fn a_visit_still_open_at_the_last_sample_ends_there() {
        let visits =
            visits(samples(3), Scripted::new(&[&[2], &[1, 2], &[1, 2]])).expect("in order");

        assert_eq!(visits, [visit(2, 0, 20), visit(1, 10, 20)]);
    }

    #[test]
    fn a_pass_within_a_visit_of_its_crossing_is_seen_from_the_visits_first_round() {
        let (tally, offsets) = matched(&vec![pass(1, 30)], &[visit(1, 10, 40)]);

        assert_eq!(
            (
                tally.passes,
                tally.seen_passes,
                tally.visits,
                tally.unmatched_visits
            ),
            (1, 1, 1, 0)
        );
        assert_eq!(offsets, [-20.0]);
    }

    #[test]
    fn a_pass_outside_every_visit_is_unseen_and_the_visit_unmatched() {
        let (tally, offsets) = matched(&vec![pass(1, 50)], &[visit(1, 10, 40)]);

        assert_eq!(
            (
                tally.passes,
                tally.seen_passes,
                tally.visits,
                tally.unmatched_visits
            ),
            (1, 0, 1, 1)
        );
        assert!(offsets.is_empty());
    }

    #[test]
    fn a_visit_to_another_crossing_sees_no_pass() {
        let (tally, _) = matched(&vec![pass(1, 30)], &[visit(2, 10, 40)]);

        assert_eq!((tally.seen_passes, tally.unmatched_visits), (0, 1));
    }

    #[test]
    fn a_pass_on_the_first_or_last_round_of_a_visit_is_seen() {
        let (tally, offsets) = matched(
            &vec![pass(1, 10), pass(2, 40)],
            &[visit(1, 10, 40), visit(2, 10, 40)],
        );

        assert_eq!((tally.seen_passes, tally.unmatched_visits), (2, 0));
        assert_eq!(offsets, [0.0, -30.0]);
    }

    #[test]
    fn the_skew_sums_every_session_and_estimates_each_share() {
        let passes = PassesBySession::from([
            (session("a"), vec![pass(1, 30), pass(2, 300)]),
            (session("b"), vec![pass(3, 30)]),
        ]);
        let visits = BTreeMap::from([
            (session("a"), vec![visit(1, 10, 40), visit(4, 500, 510)]),
            (session("b"), vec![visit(3, 20, 30)]),
        ]);

        let skew = skew(&sessions(&["a", "b"]), &passes, &visits, &bootstrap()).expect("a skew");

        assert_eq!(
            (
                skew.sessions,
                skew.passes,
                skew.visits,
                skew.seen_passes,
                skew.unmatched_visits
            ),
            (2, 3, 3, 2, 1)
        );
        let seen = skew.seen.expect("a share seen");
        let unmatched = skew.unmatched.expect("a share unmatched");
        assert_eq!((seen.value, unmatched.value), (2.0 / 3.0, 1.0 / 3.0));
        assert!(
            seen.low <= seen.value && seen.value <= seen.high,
            "{seen:?}"
        );
        assert!(
            unmatched.low <= unmatched.value && unmatched.value <= unmatched.high,
            "{unmatched:?}"
        );
        assert_eq!(
            skew.offsets,
            Some(Spread {
                p10: TimeDelta::seconds(-20),
                median: TimeDelta::seconds(-15),
                p90: TimeDelta::seconds(-10),
            })
        );
    }

    #[test]
    fn a_run_with_no_passes_and_no_visits_has_no_shares() {
        let passes = PassesBySession::from([(session("a"), vec![])]);
        let visits = BTreeMap::from([(session("a"), vec![])]);

        let skew = skew(&sessions(&["a"]), &passes, &visits, &bootstrap()).expect("a skew");

        assert_eq!(
            (skew.seen, skew.unmatched, skew.offsets),
            (None, None, None)
        );
    }

    #[test]
    fn a_session_without_visits_is_refused() {
        let passes = PassesBySession::from([(session("a"), vec![])]);

        assert_eq!(
            skew(&sessions(&["a"]), &passes, &BTreeMap::new(), &bootstrap()),
            Err(SkewError::NoVisits {
                session_id: session("a")
            })
        );
    }

    #[test]
    fn a_session_without_passes_is_refused() {
        let visits = BTreeMap::from([(session("a"), vec![])]);

        assert_eq!(
            skew(
                &sessions(&["a"]),
                &PassesBySession::new(),
                &visits,
                &bootstrap()
            ),
            Err(SkewError::NoPasses {
                session_id: session("a")
            })
        );
    }
}
