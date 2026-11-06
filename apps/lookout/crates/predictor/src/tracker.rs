use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use domain::{CrossingCompactId, Precision};

use crate::detect::Passes;
use crate::predict::{Confidence, Predictions};
use crate::state::{AlertedPass, CandidatePass, CrossingState, StateKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Transition {
    pub crossing_compact_id: CrossingCompactId,
    pub at: DateTime<Utc>,
    pub from: StateKind,
    pub to: StateKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TrackError {
    #[error("a round at {at} is behind the last round at {now}")]
    OutOfOrder {
        now: DateTime<Utc>,
        at: DateTime<Utc>,
    },
}

#[derive(Debug, Default)]
pub struct Tracker {
    now: Option<DateTime<Utc>>,
    states: BTreeMap<CrossingCompactId, CrossingState>,
}

impl Tracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn kind(&self, crossing_compact_id: CrossingCompactId) -> StateKind {
        CrossingState::kind_of(self.states.get(&crossing_compact_id))
    }

    pub fn track<P: Precision>(
        &mut self,
        at: DateTime<Utc>,
        predictions: &Predictions<P>,
        passed: &Passes,
    ) -> Result<Vec<Transition>, TrackError> {
        if let Some(now) = self.now
            && at < now
        {
            return Err(TrackError::OutOfOrder { now, at });
        }
        self.now = Some(at);

        let crossings: BTreeSet<CrossingCompactId> = self
            .states
            .keys()
            .chain(predictions.keys())
            .copied()
            .collect();

        Ok(crossings
            .into_iter()
            .flat_map(|crossing_compact_id| {
                let before = self.states.remove(&crossing_compact_id);
                let from = CrossingState::kind_of(before.as_ref());
                let (through, after) = next(
                    before,
                    predictions
                        .get(&crossing_compact_id)
                        .map(|prediction| prediction.confidence),
                    passed.contains(&crossing_compact_id),
                );
                let to = CrossingState::kind_of(after.as_ref());
                if let Some(state) = after {
                    self.states.insert(crossing_compact_id, state);
                }
                let path: Vec<StateKind> = [Some(from), through, Some(to)]
                    .into_iter()
                    .flatten()
                    .collect();
                path.windows(2)
                    .filter(|step| step[0] != step[1])
                    .map(|step| Transition {
                        crossing_compact_id,
                        at,
                        from: step[0],
                        to: step[1],
                    })
                    .collect::<Vec<_>>()
            })
            .collect())
    }
}

fn next(
    state: Option<CrossingState>,
    confidence: Option<Confidence>,
    passed: bool,
) -> (Option<StateKind>, Option<CrossingState>) {
    match (state, passed) {
        (Some(CrossingState::Alerted(state)), true) => {
            (None, Some(CrossingState::Actual(state.pass())))
        }
        (Some(passing @ CrossingState::Actual(_)), true) => (None, Some(passing)),
        (state, passed) => {
            let predicted = confidence.map(|confidence| match state {
                Some(state) => predicted(state, confidence),
                None => entered(confidence),
            });
            match (predicted, passed) {
                (Some(CrossingState::Alerted(state)), true) => (
                    Some(StateKind::AlertedPass),
                    Some(CrossingState::Actual(state.pass())),
                ),
                (predicted, _) => (None, predicted),
            }
        }
    }
}

fn entered(confidence: Confidence) -> CrossingState {
    match confidence {
        Confidence::Low => CrossingState::Candidate(CandidatePass),
        Confidence::High => CrossingState::Alerted(AlertedPass),
    }
}

fn predicted(state: CrossingState, confidence: Confidence) -> CrossingState {
    use CrossingState as State;
    match (state, confidence) {
        (State::Candidate(state), Confidence::High) => State::Alerted(state.alert()),
        (State::Alerted(state), Confidence::Low) => State::Candidate(state.withdraw()),
        (State::Actual(state), Confidence::Low) => State::Candidate(state.release()),
        (State::Actual(state), Confidence::High) => State::Alerted(state.alert()),
        (unchanged @ State::Candidate(_), Confidence::Low)
        | (unchanged @ State::Alerted(_), Confidence::High) => unchanged,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use proptest::prelude::*;

    use crate::predict::Prediction;

    use super::*;

    fn instant() -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
    }

    fn later(seconds: i64) -> DateTime<Utc> {
        instant() + TimeDelta::seconds(seconds)
    }

    fn id(n: u32) -> CrossingCompactId {
        CrossingCompactId::from(n)
    }

    fn prediction(confidence: Confidence) -> Prediction<f64> {
        Prediction {
            metres: 0.0,
            at: None,
            confidence,
        }
    }

    fn predictions(predicted: &[(u32, Confidence)]) -> Predictions<f64> {
        predicted
            .iter()
            .map(|&(n, confidence)| (id(n), prediction(confidence)))
            .collect()
    }

    fn passed(crossings: &[u32]) -> BTreeSet<CrossingCompactId> {
        crossings.iter().copied().map(id).collect()
    }

    fn none_passed() -> BTreeSet<CrossingCompactId> {
        BTreeSet::new()
    }

    fn transition(n: u32, seconds: i64, from: StateKind, to: StateKind) -> Transition {
        Transition {
            crossing_compact_id: id(n),
            at: later(seconds),
            from,
            to,
        }
    }

    type Round<'a> = (&'a [(u32, Confidence)], &'a [u32]);

    fn tracker_through(rounds: &[Round]) -> Tracker {
        let mut tracker = Tracker::new();
        for (round, &(predicted, crossings)) in (0i64..).zip(rounds) {
            tracker
                .track(
                    later(10 * round),
                    &predictions(predicted),
                    &passed(crossings),
                )
                .expect("in order");
        }
        tracker
    }

    #[test]
    fn a_crossing_never_predicted_is_out_of_reach() {
        assert_eq!(Tracker::new().kind(id(1)), StateKind::OutOfReach);
    }

    #[test]
    fn a_crossing_predicted_with_low_confidence_becomes_a_candidate() {
        let mut tracker = Tracker::new();

        let transitions = tracker
            .track(
                instant(),
                &predictions(&[(1, Confidence::Low)]),
                &none_passed(),
            )
            .expect("in order");

        assert_eq!(
            transitions,
            vec![transition(
                1,
                0,
                StateKind::OutOfReach,
                StateKind::CandidatePass
            )]
        );
    }

    #[test]
    fn a_crossing_out_of_reach_predicted_with_high_confidence_is_alerted_at_once() {
        let mut tracker = Tracker::new();

        let transitions = tracker
            .track(
                instant(),
                &predictions(&[(1, Confidence::High)]),
                &none_passed(),
            )
            .expect("in order");

        assert_eq!(
            transitions,
            vec![transition(
                1,
                0,
                StateKind::OutOfReach,
                StateKind::AlertedPass
            )]
        );
    }

    #[test]
    fn a_prediction_that_repeats_the_state_records_nothing() {
        let mut tracker = tracker_through(&[(&[(1, Confidence::High)], &[])]);

        let transitions = tracker
            .track(
                later(10),
                &predictions(&[(1, Confidence::High)]),
                &none_passed(),
            )
            .expect("in order");

        assert!(transitions.is_empty());
        assert_eq!(tracker.kind(id(1)), StateKind::AlertedPass);
    }

    #[test]
    fn an_alerted_crossing_passed_becomes_a_pass() {
        let mut tracker = tracker_through(&[(&[(1, Confidence::High)], &[])]);

        let transitions = tracker
            .track(
                later(10),
                &predictions(&[(1, Confidence::High)]),
                &passed(&[1]),
            )
            .expect("in order");

        assert_eq!(
            transitions,
            vec![transition(
                1,
                10,
                StateKind::AlertedPass,
                StateKind::ActualPass
            )]
        );
    }

    #[test]
    fn a_crossing_alerted_and_passed_in_one_round_records_both_at_that_round() {
        let mut tracker = Tracker::new();

        let transitions = tracker
            .track(
                instant(),
                &predictions(&[(1, Confidence::High)]),
                &passed(&[1]),
            )
            .expect("in order");

        assert_eq!(
            transitions,
            vec![
                transition(1, 0, StateKind::OutOfReach, StateKind::AlertedPass),
                transition(1, 0, StateKind::AlertedPass, StateKind::ActualPass),
            ]
        );
        assert_eq!(tracker.kind(id(1)), StateKind::ActualPass);
    }

    #[test]
    fn a_candidate_alerted_and_passed_in_one_round_records_both_at_that_round() {
        let mut tracker = tracker_through(&[(&[(1, Confidence::Low)], &[])]);

        let transitions = tracker
            .track(
                later(10),
                &predictions(&[(1, Confidence::High)]),
                &passed(&[1]),
            )
            .expect("in order");

        assert_eq!(
            transitions,
            vec![
                transition(1, 10, StateKind::CandidatePass, StateKind::AlertedPass),
                transition(1, 10, StateKind::AlertedPass, StateKind::ActualPass),
            ]
        );
    }

    #[test]
    fn a_pass_nobody_alerted_leaves_a_candidate_as_it_was() {
        let mut tracker = tracker_through(&[(&[(1, Confidence::Low)], &[])]);

        let transitions = tracker
            .track(
                later(10),
                &predictions(&[(1, Confidence::Low)]),
                &passed(&[1]),
            )
            .expect("in order");

        assert!(transitions.is_empty());
        assert_eq!(tracker.kind(id(1)), StateKind::CandidatePass);
    }

    #[test]
    fn a_pass_of_a_crossing_out_of_reach_records_nothing() {
        let mut tracker = Tracker::new();

        let transitions = tracker
            .track(instant(), &predictions(&[]), &passed(&[1]))
            .expect("in order");

        assert!(transitions.is_empty());
        assert_eq!(tracker.kind(id(1)), StateKind::OutOfReach);
    }

    #[test]
    fn a_crossing_stays_passed_while_it_is_passed_whatever_is_predicted() {
        let mut tracker = tracker_through(&[
            (&[(1, Confidence::High)], &[]),
            (&[(1, Confidence::High)], &[1]),
        ]);

        let transitions = tracker
            .track(later(20), &predictions(&[]), &passed(&[1]))
            .expect("in order");

        assert!(transitions.is_empty());
        assert_eq!(tracker.kind(id(1)), StateKind::ActualPass);
    }

    #[test]
    fn a_crossing_no_longer_passed_and_predicted_with_high_confidence_is_alerted_again() {
        let mut tracker = tracker_through(&[
            (&[(1, Confidence::High)], &[]),
            (&[(1, Confidence::High)], &[1]),
        ]);

        let transitions = tracker
            .track(
                later(20),
                &predictions(&[(1, Confidence::High)]),
                &none_passed(),
            )
            .expect("in order");

        assert_eq!(
            transitions,
            vec![transition(
                1,
                20,
                StateKind::ActualPass,
                StateKind::AlertedPass
            )]
        );
    }

    #[test]
    fn a_crossing_no_longer_passed_and_predicted_with_low_confidence_is_a_candidate_again() {
        let mut tracker = tracker_through(&[
            (&[(1, Confidence::High)], &[]),
            (&[(1, Confidence::High)], &[1]),
        ]);

        let transitions = tracker
            .track(
                later(20),
                &predictions(&[(1, Confidence::Low)]),
                &none_passed(),
            )
            .expect("in order");

        assert_eq!(
            transitions,
            vec![transition(
                1,
                20,
                StateKind::ActualPass,
                StateKind::CandidatePass
            )]
        );
    }

    #[test]
    fn a_crossing_no_longer_predicted_goes_out_of_reach() {
        let mut tracker = tracker_through(&[(&[(1, Confidence::High)], &[])]);

        let transitions = tracker
            .track(later(10), &predictions(&[]), &none_passed())
            .expect("in order");

        assert_eq!(
            transitions,
            vec![transition(
                1,
                10,
                StateKind::AlertedPass,
                StateKind::OutOfReach
            )]
        );
        assert_eq!(tracker.kind(id(1)), StateKind::OutOfReach);
    }

    #[test]
    fn each_crossing_moves_on_its_own_prediction() {
        let mut tracker = tracker_through(&[(&[(1, Confidence::Low), (2, Confidence::High)], &[])]);

        let transitions = tracker
            .track(
                later(10),
                &predictions(&[(1, Confidence::High), (2, Confidence::High)]),
                &none_passed(),
            )
            .expect("in order");

        assert_eq!(
            transitions,
            vec![transition(
                1,
                10,
                StateKind::CandidatePass,
                StateKind::AlertedPass
            )]
        );
        assert_eq!(tracker.kind(id(2)), StateKind::AlertedPass);
    }

    #[test]
    fn a_round_behind_the_last_is_refused_and_changes_nothing() {
        let mut tracker = Tracker::new();
        tracker
            .track(
                later(10),
                &predictions(&[(1, Confidence::High)]),
                &none_passed(),
            )
            .expect("in order");

        let refused = tracker.track(instant(), &predictions(&[]), &none_passed());

        assert_eq!(
            refused,
            Err(TrackError::OutOfOrder {
                now: later(10),
                at: instant(),
            })
        );
        assert_eq!(tracker.kind(id(1)), StateKind::AlertedPass);
    }

    fn legal(from: StateKind, to: StateKind) -> bool {
        use StateKind::*;
        matches!(
            (from, to),
            (OutOfReach, CandidatePass | AlertedPass)
                | (CandidatePass, OutOfReach | AlertedPass)
                | (AlertedPass, OutOfReach | CandidatePass | ActualPass)
                | (ActualPass, OutOfReach | CandidatePass | AlertedPass)
        )
    }

    fn confidence() -> impl Strategy<Value = Confidence> {
        prop_oneof![Just(Confidence::Low), Just(Confidence::High)]
    }

    fn round() -> impl Strategy<Value = (Predictions<f64>, BTreeSet<CrossingCompactId>)> {
        (
            prop::collection::btree_map(
                (0u32..4).prop_map(id),
                confidence().prop_map(prediction),
                0..4,
            ),
            prop::collection::btree_set((0u32..4).prop_map(id), 0..3),
        )
    }

    proptest! {
        #[test]
        fn every_crossing_runs_a_chain_of_legal_transitions_ending_in_its_state(
            rounds in prop::collection::vec(round(), 1..30)
        ) {
            let mut tracker = Tracker::new();
            let mut last: BTreeMap<CrossingCompactId, StateKind> = BTreeMap::new();

            for (round, (predictions, passed)) in (0i64..).zip(&rounds) {
                let transitions = tracker
                    .track(later(10 * round), predictions, passed)
                    .expect("in order");
                for transition in transitions {
                    let before = last
                        .get(&transition.crossing_compact_id)
                        .copied()
                        .unwrap_or(StateKind::OutOfReach);
                    prop_assert_eq!(transition.from, before);
                    prop_assert!(legal(transition.from, transition.to), "{:?}", transition);
                    if transition.to == StateKind::ActualPass {
                        prop_assert!(passed.contains(&transition.crossing_compact_id));
                    }
                    last.insert(transition.crossing_compact_id, transition.to);
                }
            }

            for n in 0..4 {
                let recorded = last.get(&id(n)).copied().unwrap_or(StateKind::OutOfReach);
                prop_assert_eq!(tracker.kind(id(n)), recorded);
            }
        }
    }
}
