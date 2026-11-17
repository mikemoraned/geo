use std::collections::BTreeMap;

use chrono::{DateTime, TimeDelta, Utc};
use domain::{CrossingCompactId, SessionId};
use itertools::Itertools;
use medallion_model::{EvaluationOutcomeRowV2, EvaluationOutcomeRowV3, EvaluationSessionRowV3};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CompactError {
    #[error("an outcome names the session {session_id}, which the sessions given do not hold")]
    UnpickedSession { session_id: SessionId },
    #[error("an outcome names the session index {session_index}, which no session given holds")]
    UnknownSessionIndex { session_index: u32 },
    #[error(
        "in {session_id}, the outcome of crossing {crossing_compact_id} has a lead of {lead:?}, \
         which is not the time from its notification to its pass"
    )]
    LeadDisagrees {
        session_id: SessionId,
        crossing_compact_id: CrossingCompactId,
        lead: Option<TimeDelta>,
    },
    #[error("in {session_id}, {span} is no count of milliseconds an outcome can hold")]
    OutOfRange {
        session_id: SessionId,
        span: TimeDelta,
    },
}

pub fn compact_outcome_rows(
    sessions: &[EvaluationSessionRowV3],
    outcomes: &[EvaluationOutcomeRowV2],
) -> Result<Vec<EvaluationOutcomeRowV3>, CompactError> {
    let sessions: BTreeMap<&SessionId, &EvaluationSessionRowV3> = sessions
        .iter()
        .map(|session| (&session.session_id, session))
        .collect();
    let compact = outcomes
        .iter()
        .map(|outcome| compact(&sessions, outcome))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(compact
        .into_iter()
        .sorted_by_key(|row| {
            (
                row.session_index,
                row.crossing_compact_id,
                row.notified_offset_ms,
                row.crossed_offset_ms,
            )
        })
        .collect())
}

fn compact(
    sessions: &BTreeMap<&SessionId, &EvaluationSessionRowV3>,
    outcome: &EvaluationOutcomeRowV2,
) -> Result<EvaluationOutcomeRowV3, CompactError> {
    let session_id = &outcome.session_id;
    let session = sessions
        .get(session_id)
        .ok_or_else(|| CompactError::UnpickedSession {
            session_id: session_id.clone(),
        })?;
    if outcome.lead != lead(outcome.notified_at, outcome.crossed_at) {
        return Err(CompactError::LeadDisagrees {
            session_id: session_id.clone(),
            crossing_compact_id: outcome.crossing_compact_id,
            lead: outcome.lead,
        });
    }
    let offset = |at: Option<DateTime<Utc>>| {
        at.map(|at| milliseconds(session_id, at - session.started_at))
            .transpose()
    };
    Ok(EvaluationOutcomeRowV3 {
        session_index: session.session_index,
        crossing_compact_id: outcome.crossing_compact_id,
        outcome: outcome.outcome,
        class: outcome.class,
        score: outcome.score,
        notified_offset_ms: offset(outcome.notified_at)?,
        crossed_offset_ms: offset(outcome.crossed_at)?,
    })
}

fn lead(
    notified_at: Option<DateTime<Utc>>,
    crossed_at: Option<DateTime<Utc>>,
) -> Option<TimeDelta> {
    Some(crossed_at? - notified_at?)
}

fn milliseconds(session_id: &SessionId, span: TimeDelta) -> Result<u32, CompactError> {
    u32::try_from(span.num_milliseconds()).map_err(|_| CompactError::OutOfRange {
        session_id: session_id.clone(),
        span,
    })
}

pub fn outcome_rows_of(
    sessions: &[EvaluationSessionRowV3],
    compact: &[EvaluationOutcomeRowV3],
) -> Result<Vec<EvaluationOutcomeRowV2>, CompactError> {
    let sessions: BTreeMap<u32, &EvaluationSessionRowV3> = sessions
        .iter()
        .map(|session| (session.session_index, session))
        .collect();
    let outcomes = compact
        .iter()
        .map(|row| {
            let session =
                sessions
                    .get(&row.session_index)
                    .ok_or(CompactError::UnknownSessionIndex {
                        session_index: row.session_index,
                    })?;
            let at = |offset: Option<u32>| {
                offset.map(|offset| session.started_at + TimeDelta::milliseconds(offset.into()))
            };
            let notified_at = at(row.notified_offset_ms);
            let crossed_at = at(row.crossed_offset_ms);
            Ok(EvaluationOutcomeRowV2 {
                session_id: session.session_id.clone(),
                crossing_compact_id: row.crossing_compact_id,
                outcome: row.outcome,
                notified_at,
                crossed_at,
                lead: lead(notified_at, crossed_at),
                class: row.class,
                score: row.score,
            })
        })
        .collect::<Result<Vec<_>, CompactError>>()?;
    Ok(outcomes
        .into_iter()
        .sorted_by(|one, other| {
            (
                &one.session_id,
                one.crossing_compact_id,
                one.notified_at,
                one.crossed_at,
            )
                .cmp(&(
                    &other.session_id,
                    other.crossing_compact_id,
                    other.notified_at,
                    other.crossed_at,
                ))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use medallion_model::{NotificationClass, OutcomeKind};
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

    fn outcome(
        index: u32,
        crossing: u32,
        notified: Option<i64>,
        crossed: Option<i64>,
    ) -> EvaluationOutcomeRowV2 {
        let notified_at = notified.map(instant);
        let crossed_at = crossed.map(instant);
        EvaluationOutcomeRowV2 {
            session_id: session(index).session_id,
            crossing_compact_id: CrossingCompactId::new(crossing),
            outcome: OutcomeKind::Credited,
            notified_at,
            crossed_at,
            lead: lead(notified_at, crossed_at),
            class: Some(NotificationClass::Hit),
            score: Some(1.0),
        }
    }

    #[test]
    fn an_outcome_keeps_its_times_as_offsets_from_its_sessions_start() {
        let compact = compact_outcome_rows(&[session(0)], &[outcome(0, 7, Some(60), Some(300))])
            .expect("compact outcomes");

        assert_eq!(
            compact,
            [EvaluationOutcomeRowV3 {
                session_index: 0,
                crossing_compact_id: CrossingCompactId::new(7),
                outcome: OutcomeKind::Credited,
                class: Some(NotificationClass::Hit),
                score: Some(1.0),
                notified_offset_ms: Some(60_000),
                crossed_offset_ms: Some(300_000),
            }]
        );
    }

    #[test]
    fn a_lead_that_is_not_the_time_from_notification_to_pass_is_refused() {
        let wrong = EvaluationOutcomeRowV2 {
            lead: Some(TimeDelta::seconds(1)),
            ..outcome(0, 7, Some(60), Some(300))
        };

        assert!(matches!(
            compact_outcome_rows(&[session(0)], &[wrong]),
            Err(CompactError::LeadDisagrees { .. })
        ));
    }

    fn outcomes() -> impl Strategy<Value = Vec<EvaluationOutcomeRowV2>> {
        prop::collection::vec(
            (
                0u32..3,
                0u32..4,
                prop::option::of(0i64..3_600),
                prop::option::of(0i64..3_600),
            )
                .prop_map(|(index, crossing, notified, crossed)| {
                    outcome(index, crossing, notified, crossed)
                }),
            0..12,
        )
        .prop_map(|outcomes| {
            outcomes
                .into_iter()
                .sorted_by(|one, other| {
                    (
                        &one.session_id,
                        one.crossing_compact_id,
                        one.notified_at,
                        one.crossed_at,
                    )
                        .cmp(&(
                            &other.session_id,
                            other.crossing_compact_id,
                            other.notified_at,
                            other.crossed_at,
                        ))
                })
                .collect()
        })
    }

    proptest! {
        #[test]
        fn rebuilding_compact_outcomes_gives_back_the_outcomes(outcomes in outcomes()) {
            let sessions: Vec<_> = (0..3).map(session).collect();

            let compact = compact_outcome_rows(&sessions, &outcomes).expect("compact outcomes");

            prop_assert_eq!(outcome_rows_of(&sessions, &compact).expect("outcomes"), outcomes);
        }
    }
}
