use std::collections::BTreeMap;

use chrono::{DateTime, TimeDelta, Utc};
use domain::{CrossingCompactId, SessionId, StateKind};
use itertools::Itertools;
use medallion_model::{
    EpisodePath, EvaluationEpisodeRowV1, EvaluationSessionRowV3, EvaluationTransitionRowV1,
    IllegalPath,
};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EpisodeError {
    #[error("a row names the session {session_id}, which the sessions given do not hold")]
    UnpickedSession { session_id: SessionId },
    #[error("an episode names the session index {session_index}, which no session given holds")]
    UnknownSessionIndex { session_index: u32 },
    #[error(
        "in {session_id}, crossing {crossing_compact_id} moves from {from:?} at {at}, \
         which is not the state its last transition entered"
    )]
    BrokenChain {
        session_id: SessionId,
        crossing_compact_id: CrossingCompactId,
        at: DateTime<Utc>,
        from: StateKind,
    },
    #[error("in {session_id}, {span} is no count of milliseconds an episode can hold")]
    OutOfRange {
        session_id: SessionId,
        span: TimeDelta,
    },
    #[error(transparent)]
    Path(#[from] IllegalPath),
}

struct Opened<'a> {
    first: &'a EvaluationTransitionRowV1,
    transitions: Vec<&'a EvaluationTransitionRowV1>,
}

pub fn episode_rows(
    sessions: &[EvaluationSessionRowV3],
    transitions: &[EvaluationTransitionRowV1],
) -> Result<Vec<EvaluationEpisodeRowV1>, EpisodeError> {
    let sessions: BTreeMap<&SessionId, &EvaluationSessionRowV3> = sessions
        .iter()
        .map(|session| (&session.session_id, session))
        .collect();
    let chains = transitions
        .iter()
        .map(|transition| {
            (
                (&transition.session_id, transition.crossing_compact_id),
                transition,
            )
        })
        .into_group_map();
    let mut episodes = chains
        .into_values()
        .map(|chain| episodes_of(&chain))
        .flatten_ok()
        .map(|opened| episode_row(&sessions, opened?))
        .collect::<Result<Vec<_>, _>>()?;
    episodes.sort_by_key(|episode| {
        (
            episode.session_index,
            episode.start_offset_ms,
            episode.crossing_compact_id,
        )
    });
    Ok(episodes)
}

fn episodes_of<'a>(
    chain: &[&'a EvaluationTransitionRowV1],
) -> Result<Vec<Opened<'a>>, EpisodeError> {
    let mut episodes: Vec<Opened<'a>> = Vec::new();
    let mut entered = StateKind::OutOfReach;
    for &transition in chain {
        if transition.from != entered {
            return Err(EpisodeError::BrokenChain {
                session_id: transition.session_id.clone(),
                crossing_compact_id: transition.crossing_compact_id,
                at: transition.at,
                from: transition.from,
            });
        }
        match episodes.last_mut() {
            Some(open) if entered != StateKind::OutOfReach => open.transitions.push(transition),
            _ => episodes.push(Opened {
                first: transition,
                transitions: vec![transition],
            }),
        }
        entered = transition.to;
    }
    Ok(episodes)
}

fn episode_row(
    sessions: &BTreeMap<&SessionId, &EvaluationSessionRowV3>,
    opened: Opened<'_>,
) -> Result<EvaluationEpisodeRowV1, EpisodeError> {
    let session_id = &opened.first.session_id;
    let session = sessions
        .get(session_id)
        .ok_or_else(|| EpisodeError::UnpickedSession {
            session_id: session_id.clone(),
        })?;
    let states: Vec<StateKind> = opened.transitions.iter().map(|row| row.to).collect();
    let gaps_ms = opened
        .transitions
        .iter()
        .tuple_windows()
        .map(|(earlier, later)| milliseconds(session_id, later.at - earlier.at))
        .collect::<Result<_, _>>()?;
    Ok(EvaluationEpisodeRowV1 {
        session_index: session.session_index,
        crossing_compact_id: opened.first.crossing_compact_id,
        start_offset_ms: milliseconds(session_id, opened.first.at - session.started_at)?,
        path: EpisodePath::of(&states)?,
        gaps_ms,
    })
}

fn milliseconds(session_id: &SessionId, span: TimeDelta) -> Result<u32, EpisodeError> {
    u32::try_from(span.num_milliseconds()).map_err(|_| EpisodeError::OutOfRange {
        session_id: session_id.clone(),
        span,
    })
}

pub fn transition_rows_of(
    sessions: &[EvaluationSessionRowV3],
    episodes: &[EvaluationEpisodeRowV1],
) -> Result<Vec<EvaluationTransitionRowV1>, EpisodeError> {
    let sessions: BTreeMap<u32, &EvaluationSessionRowV3> = sessions
        .iter()
        .map(|session| (session.session_index, session))
        .collect();
    let expanded = episodes
        .iter()
        .enumerate()
        .map(|(position, episode)| {
            let session =
                sessions
                    .get(&episode.session_index)
                    .ok_or(EpisodeError::UnknownSessionIndex {
                        session_index: episode.session_index,
                    })?;
            Ok(expanded(session, position, episode))
        })
        .flatten_ok()
        .collect::<Result<Vec<_>, EpisodeError>>()?;
    Ok(expanded
        .into_iter()
        .sorted_by(
            |(one, one_episode, one_step), (other, other_episode, other_step)| {
                (
                    &one.session_id,
                    one.at,
                    one.crossing_compact_id,
                    one_episode,
                    one_step,
                )
                    .cmp(&(
                        &other.session_id,
                        other.at,
                        other.crossing_compact_id,
                        other_episode,
                        other_step,
                    ))
            },
        )
        .map(|(row, _, _)| row)
        .collect())
}

fn expanded(
    session: &EvaluationSessionRowV3,
    position: usize,
    episode: &EvaluationEpisodeRowV1,
) -> Vec<(EvaluationTransitionRowV1, usize, usize)> {
    let start = session.started_at + TimeDelta::milliseconds(episode.start_offset_ms.into());
    let instants = std::iter::once(start).chain(episode.gaps_ms.iter().scan(start, |at, gap| {
        *at += TimeDelta::milliseconds((*gap).into());
        Some(*at)
    }));
    let states = episode.path.states();
    let froms = std::iter::once(StateKind::OutOfReach).chain(states.iter().copied());
    instants
        .zip(froms.zip(states.iter().copied()))
        .enumerate()
        .map(|(step, (at, (from, to)))| {
            (
                EvaluationTransitionRowV1 {
                    session_id: session.session_id.clone(),
                    crossing_compact_id: episode.crossing_compact_id,
                    at,
                    from,
                    to,
                },
                position,
                step,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use StateKind::*;
    use proptest::prelude::*;

    use super::*;
    use crate::fixtures::instant;

    fn session(index: u32) -> EvaluationSessionRowV3 {
        EvaluationSessionRowV3 {
            session_index: index,
            session_id: SessionId::new(format!("session-{index}")).expect("a session id"),
            started_at: instant(0),
            ended_at: instant(36_000),
            ..medallion::Row::example()
        }
    }

    fn transition(
        index: u32,
        crossing: u32,
        seconds: i64,
        from: StateKind,
        to: StateKind,
    ) -> EvaluationTransitionRowV1 {
        EvaluationTransitionRowV1 {
            session_id: session(index).session_id,
            crossing_compact_id: CrossingCompactId::new(crossing),
            at: instant(seconds),
            from,
            to,
        }
    }

    #[test]
    fn a_crossings_transitions_split_into_an_episode_per_stay_within_reach() {
        let transitions = [
            transition(0, 1, 10, OutOfReach, CandidatePass),
            transition(0, 1, 30, CandidatePass, AlertedPass),
            transition(0, 1, 30, AlertedPass, ActualPass),
            transition(0, 1, 60, ActualPass, OutOfReach),
            transition(0, 1, 90, OutOfReach, CandidatePass),
        ];

        let episodes = episode_rows(&[session(0)], &transitions).expect("episodes");

        assert_eq!(
            episodes,
            [
                EvaluationEpisodeRowV1 {
                    session_index: 0,
                    crossing_compact_id: CrossingCompactId::new(1),
                    start_offset_ms: 10_000,
                    path: EpisodePath::new("candidate_pass,alerted_pass,actual_pass,out_of_reach")
                        .expect("a path"),
                    gaps_ms: vec![20_000, 0, 30_000],
                },
                EvaluationEpisodeRowV1 {
                    session_index: 0,
                    crossing_compact_id: CrossingCompactId::new(1),
                    start_offset_ms: 90_000,
                    path: EpisodePath::new("candidate_pass").expect("a path"),
                    gaps_ms: vec![],
                },
            ]
        );
    }

    #[test]
    fn a_transition_from_a_state_its_crossing_was_not_in_is_refused() {
        let transitions = [
            transition(0, 1, 10, OutOfReach, CandidatePass),
            transition(0, 1, 20, AlertedPass, ActualPass),
        ];

        assert!(matches!(
            episode_rows(&[session(0)], &transitions),
            Err(EpisodeError::BrokenChain { .. })
        ));
    }

    #[test]
    fn a_transition_before_its_session_starts_is_refused() {
        let transitions = [transition(0, 1, -10, OutOfReach, CandidatePass)];

        assert!(matches!(
            episode_rows(&[session(0)], &transitions),
            Err(EpisodeError::OutOfRange { .. })
        ));
    }

    fn next(from: StateKind) -> impl Strategy<Value = StateKind> {
        let choices = match from {
            OutOfReach => vec![CandidatePass, AlertedPass],
            CandidatePass => vec![OutOfReach, AlertedPass],
            AlertedPass => vec![OutOfReach, CandidatePass, ActualPass],
            ActualPass => vec![OutOfReach, CandidatePass, AlertedPass],
        };
        prop::sample::select(choices)
    }

    fn chain(length: usize) -> BoxedStrategy<Vec<StateKind>> {
        (0..length).fold(Just(Vec::new()).boxed(), |states, _| {
            states
                .prop_flat_map(|states: Vec<StateKind>| {
                    let from = states.last().copied().unwrap_or(OutOfReach);
                    next(from).prop_map(move |to| {
                        let mut longer = states.clone();
                        longer.push(to);
                        longer
                    })
                })
                .boxed()
        })
    }

    fn crossing_transitions(
        index: u32,
        crossing: u32,
    ) -> impl Strategy<Value = Vec<EvaluationTransitionRowV1>> {
        (1usize..12)
            .prop_flat_map(chain)
            .prop_flat_map(|states| {
                let steps = prop::collection::vec(0i64..3, states.len());
                (Just(states), steps)
            })
            .prop_map(move |(states, steps)| {
                let froms = std::iter::once(OutOfReach).chain(states.iter().copied());
                let instants = steps.iter().scan(0, |seconds, step| {
                    *seconds += step * 10;
                    Some(*seconds)
                });
                instants
                    .zip(froms.zip(states.iter().copied()))
                    .map(|(seconds, (from, to))| transition(index, crossing, seconds, from, to))
                    .collect()
            })
    }

    fn run() -> impl Strategy<Value = (Vec<EvaluationSessionRowV3>, Vec<EvaluationTransitionRowV1>)>
    {
        prop::collection::vec(
            (0u32..3, 0u32..4)
                .prop_flat_map(|(index, crossing)| crossing_transitions(index, crossing)),
            0..6,
        )
        .prop_map(|chains| {
            let mut seen = std::collections::BTreeSet::new();
            let transitions: Vec<EvaluationTransitionRowV1> = chains
                .into_iter()
                .filter(|chain| {
                    chain.first().is_none_or(|first| {
                        seen.insert((first.session_id.clone(), first.crossing_compact_id))
                    })
                })
                .flatten()
                .enumerate()
                .sorted_by(|(one_order, one), (other_order, other)| {
                    (&one.session_id, one.at, one.crossing_compact_id, one_order).cmp(&(
                        &other.session_id,
                        other.at,
                        other.crossing_compact_id,
                        other_order,
                    ))
                })
                .map(|(_, transition)| transition)
                .collect();
            ((0..3).map(session).collect(), transitions)
        })
    }

    proptest! {
        #[test]
        fn expanding_a_runs_episodes_gives_back_its_transitions((sessions, transitions) in run()) {
            let episodes = episode_rows(&sessions, &transitions).expect("episodes");

            prop_assert_eq!(
                transition_rows_of(&sessions, &episodes).expect("transitions"),
                transitions
            );
        }
    }
}
