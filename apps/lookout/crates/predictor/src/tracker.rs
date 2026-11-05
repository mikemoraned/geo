use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use domain::CrossingCompactId;

use crate::predict::ObserveError;
use crate::state::{CandidatePass, CrossingState, PredictedPass, StateKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Judgement {
    CandidatePass,
    PredictedPass,
    ActualPass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Transition {
    pub crossing_compact_id: CrossingCompactId,
    pub at: DateTime<Utc>,
    pub from: StateKind,
    pub to: StateKind,
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

    pub fn state(&self, crossing_compact_id: CrossingCompactId) -> Option<&CrossingState> {
        self.states.get(&crossing_compact_id)
    }

    pub fn judge(
        &mut self,
        at: DateTime<Utc>,
        judgements: &BTreeMap<CrossingCompactId, Judgement>,
    ) -> Result<Vec<Transition>, ObserveError> {
        if let Some(now) = self.now
            && at < now
        {
            return Err(ObserveError::OutOfOrder { now, at });
        }
        self.now = Some(at);

        let crossings: BTreeSet<CrossingCompactId> = self
            .states
            .keys()
            .chain(judgements.keys())
            .copied()
            .collect();

        Ok(crossings
            .into_iter()
            .filter_map(|crossing_compact_id| {
                let before = self.states.remove(&crossing_compact_id);
                let from = before
                    .as_ref()
                    .map_or(StateKind::OutOfReach, CrossingState::kind);
                let after = next(before, judgements.get(&crossing_compact_id).copied(), at);
                let to = after
                    .as_ref()
                    .map_or(StateKind::OutOfReach, CrossingState::kind);
                if let Some(state) = after {
                    self.states.insert(crossing_compact_id, state);
                }
                (from != to).then_some(Transition {
                    crossing_compact_id,
                    at,
                    from,
                    to,
                })
            })
            .collect())
    }
}

fn next(
    state: Option<CrossingState>,
    judgement: Option<Judgement>,
    at: DateTime<Utc>,
) -> Option<CrossingState> {
    use CrossingState as State;
    match (state, judgement?) {
        (None, Judgement::CandidatePass | Judgement::ActualPass) => {
            Some(State::CandidatePass(CandidatePass::new(at)))
        }
        (None, Judgement::PredictedPass) => Some(State::PredictedPass(PredictedPass::new(at))),
        (Some(State::CandidatePass(state)), Judgement::PredictedPass) => {
            Some(State::PredictedPass(state.predict(at)))
        }
        (Some(State::PredictedPass(state)), Judgement::CandidatePass) => {
            Some(State::CandidatePass(state.withdraw(at)))
        }
        (Some(State::PredictedPass(state)), Judgement::ActualPass) => {
            Some(State::ActualPass(state.pass(at)))
        }
        (Some(State::ActualPass(state)), Judgement::CandidatePass) => {
            Some(State::CandidatePass(state.release(at)))
        }
        (Some(State::ActualPass(state)), Judgement::PredictedPass) => {
            Some(State::PredictedPass(state.predict(at)))
        }
        (
            Some(unchanged @ State::CandidatePass(_)),
            Judgement::CandidatePass | Judgement::ActualPass,
        )
        | (Some(unchanged @ State::PredictedPass(_)), Judgement::PredictedPass)
        | (Some(unchanged @ State::ActualPass(_)), Judgement::ActualPass) => Some(unchanged),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use proptest::prelude::*;

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

    fn judged(judgements: &[(u32, Judgement)]) -> BTreeMap<CrossingCompactId, Judgement> {
        judgements
            .iter()
            .map(|&(n, judgement)| (id(n), judgement))
            .collect()
    }

    fn nothing() -> BTreeMap<CrossingCompactId, Judgement> {
        BTreeMap::new()
    }

    fn transition(n: u32, seconds: i64, from: StateKind, to: StateKind) -> Transition {
        Transition {
            crossing_compact_id: id(n),
            at: later(seconds),
            from,
            to,
        }
    }

    fn kind(tracker: &Tracker, n: u32) -> StateKind {
        tracker
            .state(id(n))
            .map_or(StateKind::OutOfReach, CrossingState::kind)
    }

    #[test]
    fn a_crossing_never_judged_is_out_of_reach() {
        assert_eq!(kind(&Tracker::new(), 1), StateKind::OutOfReach);
    }

    #[test]
    fn a_crossing_judged_a_candidate_becomes_one() {
        let mut tracker = Tracker::new();

        let transitions = tracker
            .judge(instant(), &judged(&[(1, Judgement::CandidatePass)]))
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
        assert_eq!(kind(&tracker, 1), StateKind::CandidatePass);
    }

    #[test]
    fn a_crossing_out_of_reach_judged_predicted_is_predicted_at_once() {
        let mut tracker = Tracker::new();

        let transitions = tracker
            .judge(instant(), &judged(&[(1, Judgement::PredictedPass)]))
            .expect("in order");

        assert_eq!(
            transitions,
            vec![transition(
                1,
                0,
                StateKind::OutOfReach,
                StateKind::PredictedPass
            )]
        );
    }

    #[test]
    fn a_judgement_that_repeats_the_state_records_nothing_and_keeps_its_start() {
        let mut tracker = Tracker::new();
        tracker
            .judge(instant(), &judged(&[(1, Judgement::PredictedPass)]))
            .expect("in order");

        let transitions = tracker
            .judge(later(10), &judged(&[(1, Judgement::PredictedPass)]))
            .expect("in order");

        assert!(transitions.is_empty());
        assert_eq!(tracker.state(id(1)).expect("a state").since(), instant());
    }

    #[test]
    fn a_prediction_judged_passed_becomes_a_pass() {
        let mut tracker = Tracker::new();
        tracker
            .judge(instant(), &judged(&[(1, Judgement::PredictedPass)]))
            .expect("in order");

        let transitions = tracker
            .judge(later(10), &judged(&[(1, Judgement::ActualPass)]))
            .expect("in order");

        assert_eq!(
            transitions,
            vec![transition(
                1,
                10,
                StateKind::PredictedPass,
                StateKind::ActualPass
            )]
        );
    }

    #[test]
    fn a_pass_nobody_predicted_leaves_a_candidate_as_it_was() {
        let mut tracker = Tracker::new();
        tracker
            .judge(instant(), &judged(&[(1, Judgement::CandidatePass)]))
            .expect("in order");

        let transitions = tracker
            .judge(later(10), &judged(&[(1, Judgement::ActualPass)]))
            .expect("in order");

        assert!(transitions.is_empty());
        assert_eq!(kind(&tracker, 1), StateKind::CandidatePass);
    }

    #[test]
    fn a_pass_nobody_predicted_brings_a_crossing_out_of_reach_in_as_a_candidate() {
        let mut tracker = Tracker::new();

        let transitions = tracker
            .judge(instant(), &judged(&[(1, Judgement::ActualPass)]))
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
    fn a_crossing_no_longer_judged_goes_out_of_reach() {
        let mut tracker = Tracker::new();
        tracker
            .judge(instant(), &judged(&[(1, Judgement::PredictedPass)]))
            .expect("in order");

        let transitions = tracker.judge(later(10), &nothing()).expect("in order");

        assert_eq!(
            transitions,
            vec![transition(
                1,
                10,
                StateKind::PredictedPass,
                StateKind::OutOfReach
            )]
        );
        assert!(tracker.state(id(1)).is_none());
    }

    #[test]
    fn a_crossing_passed_and_predicted_again_is_a_new_prediction() {
        let mut tracker = Tracker::new();
        tracker
            .judge(instant(), &judged(&[(1, Judgement::PredictedPass)]))
            .expect("in order");
        tracker
            .judge(later(10), &judged(&[(1, Judgement::ActualPass)]))
            .expect("in order");

        let transitions = tracker
            .judge(later(20), &judged(&[(1, Judgement::PredictedPass)]))
            .expect("in order");

        assert_eq!(
            transitions,
            vec![transition(
                1,
                20,
                StateKind::ActualPass,
                StateKind::PredictedPass
            )]
        );
    }

    #[test]
    fn each_crossing_moves_on_its_own_judgement() {
        let mut tracker = Tracker::new();
        tracker
            .judge(
                instant(),
                &judged(&[(1, Judgement::CandidatePass), (2, Judgement::PredictedPass)]),
            )
            .expect("in order");

        let transitions = tracker
            .judge(
                later(10),
                &judged(&[(1, Judgement::PredictedPass), (2, Judgement::PredictedPass)]),
            )
            .expect("in order");

        assert_eq!(
            transitions,
            vec![transition(
                1,
                10,
                StateKind::CandidatePass,
                StateKind::PredictedPass
            )]
        );
        assert_eq!(kind(&tracker, 2), StateKind::PredictedPass);
    }

    #[test]
    fn a_judgement_behind_the_last_is_refused_and_changes_nothing() {
        let mut tracker = Tracker::new();
        tracker
            .judge(later(10), &judged(&[(1, Judgement::PredictedPass)]))
            .expect("in order");

        let refused = tracker.judge(instant(), &nothing());

        assert_eq!(
            refused,
            Err(ObserveError::OutOfOrder {
                now: later(10),
                at: instant(),
            })
        );
        assert_eq!(kind(&tracker, 1), StateKind::PredictedPass);
    }

    fn legal(from: StateKind, to: StateKind) -> bool {
        use StateKind::*;
        matches!(
            (from, to),
            (OutOfReach, CandidatePass | PredictedPass)
                | (CandidatePass, OutOfReach | PredictedPass)
                | (PredictedPass, OutOfReach | CandidatePass | ActualPass)
                | (ActualPass, OutOfReach | CandidatePass | PredictedPass)
        )
    }

    fn judgement() -> impl Strategy<Value = Judgement> {
        prop_oneof![
            Just(Judgement::CandidatePass),
            Just(Judgement::PredictedPass),
            Just(Judgement::ActualPass),
        ]
    }

    fn rounds() -> impl Strategy<Value = Vec<BTreeMap<CrossingCompactId, Judgement>>> {
        prop::collection::vec(
            prop::collection::btree_map((0u32..4).prop_map(id), judgement(), 0..4),
            1..30,
        )
    }

    proptest! {
        #[test]
        fn every_crossing_runs_a_chain_of_legal_transitions_ending_in_its_state(rounds in rounds()) {
            let mut tracker = Tracker::new();
            let mut last: BTreeMap<CrossingCompactId, StateKind> = BTreeMap::new();

            for (round, judgements) in (0i64..).zip(&rounds) {
                let transitions = tracker.judge(later(10 * round), judgements).expect("in order");
                for transition in transitions {
                    let before = last
                        .get(&transition.crossing_compact_id)
                        .copied()
                        .unwrap_or(StateKind::OutOfReach);
                    prop_assert_eq!(transition.from, before);
                    prop_assert!(legal(transition.from, transition.to), "{:?}", transition);
                    last.insert(transition.crossing_compact_id, transition.to);
                }
            }

            for n in 0..4 {
                let recorded = last.get(&id(n)).copied().unwrap_or(StateKind::OutOfReach);
                prop_assert_eq!(kind(&tracker, n), recorded);
            }
        }
    }
}
