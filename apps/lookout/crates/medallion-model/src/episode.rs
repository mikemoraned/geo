use domain::{CrossingCompactId, StateKind};
use hashfn::hashfn;
use medallion::{DatasetSpec, RUN_DATE, RUN_ID, Row, layers};
use nutype::nutype;
use serde::de::IntoDeserializer;
use serde::de::value::Error as NameError;
use serde::{Deserialize, Serialize};

pub const EVALUATION_EPISODE_V1: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned_within("evaluation_episode_v1", RUN_DATE, &[RUN_ID]);

const SEPARATOR: char = ',';

#[nutype(
    validate(with = path_is_legal, error = IllegalPath),
    derive(
        Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Display, FromStr, TryFrom, Into, AsRef,
        Serialize, Deserialize
    )
)]
pub struct EpisodePath(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "`{0}` is not a path: a path names, comma-separated, the states a crossing enters after \
     out_of_reach, each a legal step from the last, with out_of_reach only at the end"
)]
pub struct IllegalPath(String);

#[hashfn]
pub fn path_is_legal(text: &str) -> Result<(), IllegalPath> {
    let illegal = || IllegalPath(text.to_string());
    let states = states_named(text).map_err(|_| illegal())?;
    let (_, entered) = states.split_last().ok_or_else(illegal)?;
    let mut steps = std::iter::once(StateKind::OutOfReach)
        .chain(states.iter().copied())
        .zip(states.iter().copied());
    if entered.contains(&StateKind::OutOfReach) || !steps.all(|(from, to)| legal(from, to)) {
        Err(illegal())
    } else {
        Ok(())
    }
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

fn states_named(text: &str) -> Result<Vec<StateKind>, NameError> {
    text.split(SEPARATOR)
        .map(|name| StateKind::deserialize(name.into_deserializer()))
        .collect()
}

fn name_of(state: StateKind) -> &'static str {
    match state {
        StateKind::OutOfReach => "out_of_reach",
        StateKind::CandidatePass => "candidate_pass",
        StateKind::AlertedPass => "alerted_pass",
        StateKind::ActualPass => "actual_pass",
    }
}

impl EpisodePath {
    pub fn new(text: impl Into<String>) -> Result<Self, IllegalPath> {
        Self::try_new(text)
    }

    pub fn of(states: &[StateKind]) -> Result<Self, IllegalPath> {
        let names: Vec<&str> = states.iter().copied().map(name_of).collect();
        Self::new(names.join(&SEPARATOR.to_string()))
    }

    pub fn states(&self) -> Vec<StateKind> {
        #[expect(
            clippy::unwrap_used,
            reason = "the constructor refuses a name that does not parse"
        )]
        states_named(self.as_ref()).unwrap()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationEpisodeRowV1 {
    pub session_index: u32,
    pub crossing_compact_id: CrossingCompactId,
    pub start_offset_ms: u32,
    pub path: EpisodePath,
    pub gaps_ms: Vec<u32>,
}

impl Row for EvaluationEpisodeRowV1 {
    type Layer = layers::Gold;
    const DATASET: DatasetSpec<Self::Layer> = EVALUATION_EPISODE_V1;

    #[expect(
        clippy::unwrap_used,
        reason = "each value here is a constant that its type accepts"
    )]
    fn example() -> Self {
        Self {
            session_index: 0,
            crossing_compact_id: CrossingCompactId::new(1),
            start_offset_ms: 0,
            path: EpisodePath::of(&[StateKind::CandidatePass, StateKind::OutOfReach]).unwrap(),
            gaps_ms: vec![10_000],
        }
    }
}

#[cfg(test)]
mod tests {
    use StateKind::*;
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn a_path_is_written_in_the_names_gold_gives_the_states() {
        let path =
            EpisodePath::of(&[CandidatePass, AlertedPass, ActualPass, OutOfReach]).expect("a path");

        assert_eq!(
            path.as_ref(),
            "candidate_pass,alerted_pass,actual_pass,out_of_reach"
        );
    }

    #[test]
    fn a_path_still_within_reach_at_the_end_need_not_leave_it() {
        assert!(EpisodePath::new("candidate_pass").is_ok());
        assert!(EpisodePath::new("alerted_pass,candidate_pass").is_ok());
    }

    #[test]
    fn a_path_that_is_no_chain_of_legal_steps_is_refused() {
        for text in [
            "",
            "out_of_reach",
            "actual_pass",
            "candidate_pass,actual_pass",
            "candidate_pass,candidate_pass",
            "candidate_pass,out_of_reach,candidate_pass",
            "candidate_pass,somewhere",
            "Candidate_Pass",
        ] {
            assert_eq!(
                EpisodePath::new(text),
                Err(IllegalPath(text.to_string())),
                "{text:?}"
            );
        }
    }

    fn state() -> impl Strategy<Value = StateKind> {
        prop_oneof![
            Just(OutOfReach),
            Just(CandidatePass),
            Just(AlertedPass),
            Just(ActualPass)
        ]
    }

    proptest! {
        #[test]
        fn a_path_gives_back_the_states_it_was_made_of(
            states in prop::collection::vec(state(), 1..8)
        ) {
            if let Ok(path) = EpisodePath::of(&states) {
                prop_assert_eq!(path.states(), states);
            }
        }
    }
}
