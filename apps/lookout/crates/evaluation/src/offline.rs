use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use domain::{CrossingCompactId, CrossingId, Precision, SessionId};
use medallion_model::TrainSessionCrossingRowV1;
use predictor::{DetectPasses, Event, ObserveError, Passes};

pub trait RecordedPass {
    fn session_id(&self) -> &SessionId;
    fn crossing_id(&self) -> &CrossingId;
    fn crossed_at(&self) -> DateTime<Utc>;
}

impl RecordedPass for TrainSessionCrossingRowV1 {
    fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    fn crossing_id(&self) -> &CrossingId {
        &self.crossing_id
    }

    fn crossed_at(&self) -> DateTime<Utc> {
        self.crossed_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfflineDetector {
    passes: SessionPasses,
    now: Option<DateTime<Utc>>,
    passed: Passes,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OfflineError {
    #[error("a pass names the crossing {crossing_id}, which the crossings given do not hold")]
    UnknownCrossing { crossing_id: CrossingId },
    #[error("a pass names the session {session_id}, which the sessions given do not hold")]
    UnknownSession { session_id: SessionId },
}

impl OfflineDetector {
    pub fn per_session<R: RecordedPass>(
        sessions: impl IntoIterator<Item = SessionId>,
        passes: impl IntoIterator<Item = R>,
        compact_ids: &BTreeMap<CrossingId, CrossingCompactId>,
    ) -> Result<BTreeMap<SessionId, Self>, OfflineError> {
        Ok(passes_by_session(sessions, passes, compact_ids)?
            .into_iter()
            .map(|(session_id, passes)| (session_id, Self::new(passes)))
            .collect())
    }

    fn new(passes: SessionPasses) -> Self {
        Self {
            passes,
            now: None,
            passed: Passes::new(),
        }
    }
}

pub(crate) type SessionPasses = Vec<(DateTime<Utc>, CrossingCompactId)>;

pub(crate) fn passes_by_session<R: RecordedPass>(
    sessions: impl IntoIterator<Item = SessionId>,
    passes: impl IntoIterator<Item = R>,
    compact_ids: &BTreeMap<CrossingId, CrossingCompactId>,
) -> Result<BTreeMap<SessionId, SessionPasses>, OfflineError> {
    let mut grouped: BTreeMap<SessionId, SessionPasses> = sessions
        .into_iter()
        .map(|session_id| (session_id, Vec::new()))
        .collect();
    for pass in passes {
        let compact_id =
            *compact_ids
                .get(pass.crossing_id())
                .ok_or_else(|| OfflineError::UnknownCrossing {
                    crossing_id: pass.crossing_id().clone(),
                })?;
        grouped
            .get_mut(pass.session_id())
            .ok_or_else(|| OfflineError::UnknownSession {
                session_id: pass.session_id().clone(),
            })?
            .push((pass.crossed_at(), compact_id));
    }
    Ok(grouped)
}

impl<P: Precision> DetectPasses<P> for OfflineDetector {
    fn observe(&mut self, event: Event<P>) -> Result<(), ObserveError> {
        let at = match event {
            Event::Sampled(sample) => sample.t,
            Event::Elapsed(at) => at,
        };
        if let Some(now) = self.now
            && at < now
        {
            return Err(ObserveError::OutOfOrder { now, at });
        }
        self.passed = self
            .passes
            .iter()
            .filter(|&&(crossed_at, _)| {
                crossed_at <= at && self.now.is_none_or(|now| crossed_at > now)
            })
            .map(|&(_, compact_id)| compact_id)
            .collect();
        self.now = Some(at);
        Ok(())
    }

    fn passed(&self) -> &Passes {
        &self.passed
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use domain::Sample;
    use medallion::Row;
    use proptest::prelude::*;

    use super::*;
    use domain::places::DRESDEN_HBF;

    struct Pass {
        session_id: SessionId,
        crossing_id: CrossingId,
        crossed_at: DateTime<Utc>,
    }

    impl RecordedPass for Pass {
        fn session_id(&self) -> &SessionId {
            &self.session_id
        }

        fn crossing_id(&self) -> &CrossingId {
            &self.crossing_id
        }

        fn crossed_at(&self) -> DateTime<Utc> {
            self.crossed_at
        }
    }

    fn instant(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
            + TimeDelta::seconds(seconds)
    }

    fn session(name: &str) -> SessionId {
        SessionId::new(format!("session-{name}")).expect("a session id")
    }

    fn crossing(n: u32) -> CrossingId {
        CrossingId::new(format!("crossing-{n}")).expect("a crossing id")
    }

    fn compact_ids(crossings: u32) -> BTreeMap<CrossingId, CrossingCompactId> {
        (0..crossings)
            .map(|n| (crossing(n), CrossingCompactId::new(n)))
            .collect()
    }

    fn pass_in(name: &str, n: u32, seconds: i64) -> Pass {
        Pass {
            session_id: session(name),
            crossing_id: crossing(n),
            crossed_at: instant(seconds),
        }
    }

    fn pass(n: u32, seconds: i64) -> Pass {
        pass_in("a", n, seconds)
    }

    fn detector(passes: Vec<Pass>) -> OfflineDetector {
        OfflineDetector::per_session([session("a")], passes, &compact_ids(10))
            .expect("known sessions and crossings")
            .remove(&session("a"))
            .expect("a detector for the session")
    }

    fn sampled(seconds: i64) -> Event<f64> {
        Event::Sampled(
            Sample::at(instant(seconds), DRESDEN_HBF.y(), DRESDEN_HBF.x()).expect("a sample"),
        )
    }

    fn passed_at(detector: &mut OfflineDetector, event: Event<f64>) -> Vec<u32> {
        detector.observe(event).expect("events in order");
        DetectPasses::<f64>::passed(detector)
            .iter()
            .map(CrossingCompactId::get)
            .collect()
    }

    fn rounds(detector: &mut OfflineDetector, instants: &[i64]) -> Vec<Vec<u32>> {
        instants
            .iter()
            .map(|&seconds| passed_at(detector, sampled(seconds)))
            .collect()
    }

    #[test]
    fn a_pass_is_reported_at_the_first_sample_at_or_after_it_and_at_no_other() {
        let mut detector = detector(vec![pass(1, 5), pass(2, 20)]);

        assert_eq!(
            rounds(&mut detector, &[0, 10, 20, 30]),
            [vec![], vec![1], vec![2], vec![]]
        );
    }

    #[test]
    fn passes_between_two_samples_are_reported_together() {
        let mut detector = detector(vec![pass(1, 3), pass(2, 7)]);

        assert_eq!(rounds(&mut detector, &[0, 10]), [vec![], vec![1, 2]]);
    }

    #[test]
    fn a_pass_before_the_first_sample_is_reported_at_it() {
        let mut detector = detector(vec![pass(1, -5)]);

        assert_eq!(passed_at(&mut detector, sampled(0)), [1]);
    }

    #[test]
    fn time_passing_without_a_sample_reports_a_pass_as_a_sample_would() {
        let mut detector = detector(vec![pass(1, 5)]);

        passed_at(&mut detector, sampled(0));

        assert_eq!(passed_at(&mut detector, Event::Elapsed(instant(10))), [1]);
    }

    #[test]
    fn nothing_is_reported_before_the_first_event() {
        let detector = detector(vec![pass(1, -5)]);

        assert!(DetectPasses::<f64>::passed(&detector).is_empty());
    }

    #[test]
    fn an_event_behind_the_last_is_refused() {
        let mut detector = detector(vec![pass(1, 5)]);
        passed_at(&mut detector, sampled(10));

        let refused = DetectPasses::<f64>::observe(&mut detector, sampled(9));

        assert_eq!(
            refused,
            Err(ObserveError::OutOfOrder {
                now: instant(10),
                at: instant(9)
            })
        );
    }

    #[test]
    fn each_session_reports_only_its_own_passes() {
        let mut detectors = OfflineDetector::per_session(
            [session("a"), session("b")],
            vec![pass_in("a", 1, 5), pass_in("b", 2, 5)],
            &compact_ids(10),
        )
        .expect("known sessions and crossings");

        let reported: BTreeMap<SessionId, Vec<Vec<u32>>> = detectors
            .iter_mut()
            .map(|(session_id, detector)| (session_id.clone(), rounds(detector, &[0, 10])))
            .collect();

        assert_eq!(
            reported,
            BTreeMap::from([
                (session("a"), vec![vec![], vec![1]]),
                (session("b"), vec![vec![], vec![2]]),
            ])
        );
    }

    #[test]
    fn a_session_with_no_passes_reports_nothing() {
        let mut detectors = OfflineDetector::per_session(
            [session("a"), session("b")],
            vec![pass_in("a", 1, 5)],
            &compact_ids(10),
        )
        .expect("known sessions and crossings");
        let mut quiet = detectors
            .remove(&session("b"))
            .expect("a detector for the session");

        assert!(rounds(&mut quiet, &[0, 10]).iter().all(Vec::is_empty));
    }

    #[test]
    fn a_pass_of_a_session_not_given_is_refused() {
        let refused = OfflineDetector::per_session(
            [session("a")],
            vec![pass_in("b", 1, 5)],
            &compact_ids(10),
        );

        assert_eq!(
            refused,
            Err(OfflineError::UnknownSession {
                session_id: session("b")
            })
        );
    }

    #[test]
    fn a_pass_of_a_crossing_not_given_is_refused() {
        let refused =
            OfflineDetector::per_session([session("a")], vec![pass(11, 5)], &compact_ids(10));

        assert_eq!(
            refused,
            Err(OfflineError::UnknownCrossing {
                crossing_id: crossing(11)
            })
        );
    }

    #[test]
    fn a_train_session_crossing_row_is_a_recorded_pass() {
        let row = TrainSessionCrossingRowV1 {
            session_id: session("a"),
            crossing_id: crossing(1),
            crossed_at: instant(5),
            ..TrainSessionCrossingRowV1::example()
        };
        let mut detector = OfflineDetector::per_session([session("a")], [row], &compact_ids(10))
            .expect("known sessions and crossings")
            .remove(&session("a"))
            .expect("a detector for the session");

        assert_eq!(rounds(&mut detector, &[0, 10]), [vec![], vec![1]]);
    }

    proptest! {
        #[test]
        fn each_pass_is_reported_once_however_the_samples_fall(
            passes in prop::collection::btree_map(0u32..10, 0i64..600, 0..10),
            steps in prop::collection::vec(0i64..60, 1..40),
        ) {
            let mut detector = detector(
                passes.iter().map(|(&n, &seconds)| pass(n, seconds)).collect(),
            );
            let instants: Vec<i64> = steps
                .iter()
                .scan(0, |at, step| {
                    *at += step;
                    Some(*at)
                })
                .chain([steps.iter().sum::<i64>().max(600)])
                .collect();

            let mut reported: Vec<u32> = rounds(&mut detector, &instants)
                .into_iter()
                .flatten()
                .collect();
            reported.sort_unstable();
            let expected: Vec<u32> = passes.keys().copied().collect();

            prop_assert_eq!(reported, expected);
        }
    }
}
