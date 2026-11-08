use std::collections::BTreeMap;

use chrono::TimeDelta;
use domain::{CrossingCompactId, SessionId};
use medallion_model::{
    EvaluationOutcomeRowV1, EvaluationSessionRowV1, EvaluationSummaryRowV1,
    EvaluationTransitionRowV1, NotificationClass, OutcomeKind,
};
use predictor::Transition;

use crate::score::{Class, Credit, NotAScore, Outcome, Score, Verdict};
use crate::summarise::{Estimate, LeadTimes, Summary};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RowsError {
    #[error("a row names the session {session_id}, which the sessions given do not hold")]
    UnpickedSession { session_id: SessionId },
    #[error(
        "the {outcome:?} outcome of crossing {crossing_compact_id} in {session_id} lacks its {missing}"
    )]
    Incomplete {
        session_id: SessionId,
        crossing_compact_id: CrossingCompactId,
        outcome: OutcomeKind,
        missing: &'static str,
    },
    #[error(transparent)]
    Score(#[from] NotAScore),
}

pub fn transition_rows(
    replayed: &BTreeMap<SessionId, Vec<Transition>>,
) -> Vec<EvaluationTransitionRowV1> {
    replayed
        .iter()
        .flat_map(|(session_id, transitions)| {
            transitions
                .iter()
                .map(|transition| EvaluationTransitionRowV1 {
                    session_id: session_id.clone(),
                    crossing_compact_id: transition.crossing_compact_id,
                    at: transition.at,
                    from: transition.from,
                    to: transition.to,
                })
        })
        .collect()
}

pub fn replayed_from(
    sessions: &[EvaluationSessionRowV1],
    rows: &[EvaluationTransitionRowV1],
) -> Result<BTreeMap<SessionId, Vec<Transition>>, RowsError> {
    let mut replayed = every_session(sessions);
    for row in rows {
        session_of(&mut replayed, &row.session_id)?.push(Transition {
            crossing_compact_id: row.crossing_compact_id,
            at: row.at,
            from: row.from,
            to: row.to,
        });
    }
    Ok(replayed)
}

pub fn outcome_rows(scored: &BTreeMap<SessionId, Vec<Outcome>>) -> Vec<EvaluationOutcomeRowV1> {
    scored
        .iter()
        .flat_map(|(session_id, outcomes)| {
            outcomes
                .iter()
                .map(|outcome| outcome_row(session_id.clone(), outcome))
        })
        .collect()
}

pub fn scored_from(
    sessions: &[EvaluationSessionRowV1],
    rows: &[EvaluationOutcomeRowV1],
) -> Result<BTreeMap<SessionId, Vec<Outcome>>, RowsError> {
    let mut scored = every_session(sessions);
    for row in rows {
        let outcome = outcome_of(row)?;
        session_of(&mut scored, &row.session_id)?.push(outcome);
    }
    Ok(scored)
}

pub fn summary_row(summary: &Summary) -> EvaluationSummaryRowV1 {
    let value = |estimate: Option<Estimate>| estimate.map(|estimate| estimate.value);
    let low = |estimate: Option<Estimate>| estimate.map(|estimate| estimate.low);
    let high = |estimate: Option<Estimate>| estimate.map(|estimate| estimate.high);
    let lead = |pick: fn(&LeadTimes) -> TimeDelta| {
        summary
            .lead_times
            .as_ref()
            .map(|lead_times| pick(lead_times).num_milliseconds())
    };
    EvaluationSummaryRowV1 {
        sessions: summary.sessions as u64,
        hours: summary.hours,
        passes: summary.passes as u64,
        notifications: summary.notifications as u64,
        false_alarms: summary.false_alarms as u64,
        repeats: summary.repeats as u64,
        misses: summary.misses as u64,
        precision: value(summary.precision),
        precision_low: low(summary.precision),
        precision_high: high(summary.precision),
        recall: value(summary.recall),
        recall_low: low(summary.recall),
        recall_high: high(summary.recall),
        false_alarms_per_hour: value(summary.false_alarms_per_hour),
        false_alarms_per_hour_low: low(summary.false_alarms_per_hour),
        false_alarms_per_hour_high: high(summary.false_alarms_per_hour),
        lead_p10_ms: lead(|lead_times| lead_times.p10),
        lead_median_ms: lead(|lead_times| lead_times.median),
        lead_p90_ms: lead(|lead_times| lead_times.p90),
    }
}

fn every_session<T>(sessions: &[EvaluationSessionRowV1]) -> BTreeMap<SessionId, Vec<T>> {
    sessions
        .iter()
        .map(|session| (session.session_id.clone(), Vec::new()))
        .collect()
}

fn session_of<'a, T>(
    by_session: &'a mut BTreeMap<SessionId, Vec<T>>,
    session_id: &SessionId,
) -> Result<&'a mut Vec<T>, RowsError> {
    by_session
        .get_mut(session_id)
        .ok_or_else(|| RowsError::UnpickedSession {
            session_id: session_id.clone(),
        })
}

fn outcome_row(session_id: SessionId, outcome: &Outcome) -> EvaluationOutcomeRowV1 {
    match *outcome {
        Outcome::Notification {
            crossing_compact_id,
            notified_at,
            verdict,
        } => {
            let (kind, credit, score) = match verdict {
                Verdict::Credited { credit, score } => {
                    (OutcomeKind::Credited, Some(credit), Some(score.get()))
                }
                Verdict::Repeat(credit) => (OutcomeKind::Repeat, Some(credit), Some(0.0)),
                Verdict::FalseAlarm => (OutcomeKind::FalseAlarm, None, Some(0.0)),
                Verdict::Unscored(credit) => (OutcomeKind::Unscored, Some(credit), None),
            };
            EvaluationOutcomeRowV1 {
                session_id,
                crossing_compact_id,
                outcome: kind,
                notified_at: Some(notified_at),
                crossed_at: credit.map(|credit| credit.crossed_at),
                lead_ms: credit.map(|credit| credit.lead.num_milliseconds()),
                class: credit.map(|credit| class_name(credit.class)),
                score,
            }
        }
        Outcome::Miss {
            crossing_compact_id,
            crossed_at,
        } => EvaluationOutcomeRowV1 {
            session_id,
            crossing_compact_id,
            outcome: OutcomeKind::Miss,
            notified_at: None,
            crossed_at: Some(crossed_at),
            lead_ms: None,
            class: None,
            score: Some(0.0),
        },
    }
}

fn outcome_of(row: &EvaluationOutcomeRowV1) -> Result<Outcome, RowsError> {
    let needs = |missing: &'static str| RowsError::Incomplete {
        session_id: row.session_id.clone(),
        crossing_compact_id: row.crossing_compact_id,
        outcome: row.outcome,
        missing,
    };
    let crossed_at = || row.crossed_at.ok_or_else(|| needs("crossed_at"));
    let credit = || -> Result<Credit, RowsError> {
        Ok(Credit {
            crossed_at: crossed_at()?,
            lead: TimeDelta::milliseconds(row.lead_ms.ok_or_else(|| needs("lead_ms"))?),
            class: class_kind(row.class.ok_or_else(|| needs("class"))?),
        })
    };
    let notified = |verdict: Verdict| -> Result<Outcome, RowsError> {
        Ok(Outcome::Notification {
            crossing_compact_id: row.crossing_compact_id,
            notified_at: row.notified_at.ok_or_else(|| needs("notified_at"))?,
            verdict,
        })
    };
    match row.outcome {
        OutcomeKind::Credited => notified(Verdict::Credited {
            credit: credit()?,
            score: Score::new(row.score.ok_or_else(|| needs("score"))?)?,
        }),
        OutcomeKind::Repeat => notified(Verdict::Repeat(credit()?)),
        OutcomeKind::FalseAlarm => notified(Verdict::FalseAlarm),
        OutcomeKind::Unscored => notified(Verdict::Unscored(credit()?)),
        OutcomeKind::Miss => Ok(Outcome::Miss {
            crossing_compact_id: row.crossing_compact_id,
            crossed_at: crossed_at()?,
        }),
    }
}

fn class_name(class: Class) -> NotificationClass {
    match class {
        Class::Hit => NotificationClass::Hit,
        Class::LastMoment => NotificationClass::LastMoment,
        Class::TooEarly => NotificationClass::TooEarly,
    }
}

fn class_kind(name: NotificationClass) -> Class {
    match name {
        NotificationClass::Hit => Class::Hit,
        NotificationClass::LastMoment => Class::LastMoment,
        NotificationClass::TooEarly => Class::TooEarly,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeDelta, Utc};
    use domain::StateKind;
    use medallion::Row;
    use proptest::prelude::*;

    use super::*;

    fn instant(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
            + TimeDelta::seconds(seconds)
    }

    fn session(name: &str) -> SessionId {
        SessionId::new(format!("session-{name}")).expect("a session id")
    }

    fn picked(names: &[&str]) -> Vec<EvaluationSessionRowV1> {
        names
            .iter()
            .map(|&name| EvaluationSessionRowV1 {
                session_id: session(name),
                ..EvaluationSessionRowV1::example()
            })
            .collect()
    }

    fn kind() -> impl Strategy<Value = StateKind> {
        prop_oneof![
            Just(StateKind::OutOfReach),
            Just(StateKind::CandidatePass),
            Just(StateKind::AlertedPass),
            Just(StateKind::ActualPass),
        ]
    }

    fn transitions() -> impl Strategy<Value = Vec<Transition>> {
        prop::collection::vec((0u32..3, 0i64..600, kind(), kind()), 0..8).prop_map(|steps| {
            steps
                .into_iter()
                .map(|(crossing, seconds, from, to)| Transition {
                    crossing_compact_id: CrossingCompactId::new(crossing),
                    at: instant(seconds),
                    from,
                    to,
                })
                .collect()
        })
    }

    proptest! {
        #[test]
        fn transitions_survive_a_round_trip_through_rows(
            replayed in prop::collection::vec(transitions(), 3)
        ) {
            let replayed: BTreeMap<SessionId, Vec<Transition>> = ["a", "b", "c"]
                .into_iter()
                .map(session)
                .zip(replayed)
                .collect();

            let rows = transition_rows(&replayed);

            prop_assert_eq!(replayed_from(&picked(&["a", "b", "c"]), &rows), Ok(replayed));
        }
    }

    #[test]
    fn a_transition_of_a_session_not_picked_is_refused() {
        let rows = transition_rows(&BTreeMap::from([(
            session("b"),
            vec![Transition {
                crossing_compact_id: CrossingCompactId::new(1),
                at: instant(0),
                from: StateKind::OutOfReach,
                to: StateKind::AlertedPass,
            }],
        )]));

        assert_eq!(
            replayed_from(&picked(&["a"]), &rows),
            Err(RowsError::UnpickedSession {
                session_id: session("b")
            })
        );
    }

    fn credit(class: Class, lead: i64) -> Credit {
        Credit {
            crossed_at: instant(1_000),
            lead: TimeDelta::seconds(lead),
            class,
        }
    }

    fn notified(crossing: u32, seconds: i64, verdict: Verdict) -> Outcome {
        Outcome::Notification {
            crossing_compact_id: CrossingCompactId::new(crossing),
            notified_at: instant(seconds),
            verdict,
        }
    }

    fn every_kind() -> BTreeMap<SessionId, Vec<Outcome>> {
        BTreeMap::from([
            (
                session("a"),
                vec![
                    notified(
                        1,
                        10,
                        Verdict::Credited {
                            credit: credit(Class::TooEarly, 330),
                            score: Score::new(0.525).expect("a score"),
                        },
                    ),
                    notified(1, 20, Verdict::Repeat(credit(Class::Hit, 290))),
                    notified(2, 30, Verdict::FalseAlarm),
                    notified(3, 40, Verdict::Unscored(credit(Class::LastMoment, 10))),
                    Outcome::Miss {
                        crossing_compact_id: CrossingCompactId::new(4),
                        crossed_at: instant(500),
                    },
                ],
            ),
            (session("b"), vec![]),
        ])
    }

    #[test]
    fn outcomes_of_every_kind_survive_a_round_trip_through_rows() {
        let scored = every_kind();

        let rows = outcome_rows(&scored);

        assert_eq!(scored_from(&picked(&["a", "b"]), &rows), Ok(scored));
    }

    #[test]
    fn an_outcome_rows_score_is_what_the_outcome_earned() {
        let rows = outcome_rows(&every_kind());

        let earned: Vec<(OutcomeKind, Option<NotificationClass>, Option<f64>)> = rows
            .iter()
            .map(|row| (row.outcome, row.class, row.score))
            .collect();
        assert_eq!(
            earned,
            [
                (
                    OutcomeKind::Credited,
                    Some(NotificationClass::TooEarly),
                    Some(0.525)
                ),
                (OutcomeKind::Repeat, Some(NotificationClass::Hit), Some(0.0)),
                (OutcomeKind::FalseAlarm, None, Some(0.0)),
                (
                    OutcomeKind::Unscored,
                    Some(NotificationClass::LastMoment),
                    None
                ),
                (OutcomeKind::Miss, None, Some(0.0)),
            ]
        );
        assert_eq!(rows[0].lead_ms, Some(330_000));
    }

    #[test]
    fn a_credited_outcome_row_without_its_pass_is_refused() {
        let mut rows = outcome_rows(&every_kind());
        rows[0].crossed_at = None;

        assert_eq!(
            scored_from(&picked(&["a", "b"]), &rows),
            Err(RowsError::Incomplete {
                session_id: session("a"),
                crossing_compact_id: CrossingCompactId::new(1),
                outcome: OutcomeKind::Credited,
                missing: "crossed_at",
            })
        );
    }

    #[test]
    fn an_outcome_row_scoring_outside_0_to_1_is_refused() {
        let mut rows = outcome_rows(&every_kind());
        rows[0].score = Some(1.5);

        assert!(matches!(
            scored_from(&picked(&["a", "b"]), &rows),
            Err(RowsError::Score(_))
        ));
    }

    #[test]
    fn an_outcome_of_a_session_not_picked_is_refused() {
        let rows = outcome_rows(&every_kind());

        assert_eq!(
            scored_from(&picked(&["b"]), &rows),
            Err(RowsError::UnpickedSession {
                session_id: session("a")
            })
        );
    }

    #[test]
    fn a_summary_row_carries_every_count_and_estimate() {
        let estimate = |value: f64| {
            Some(Estimate {
                value,
                low: value - 0.1,
                high: value + 0.1,
            })
        };
        let summary = Summary {
            sessions: 2,
            hours: 1.5,
            passes: 3,
            notifications: 4,
            false_alarms: 1,
            repeats: 1,
            misses: 1,
            precision: estimate(0.4),
            recall: estimate(0.5),
            false_alarms_per_hour: None,
            lead_times: Some(LeadTimes {
                p10: TimeDelta::seconds(60),
                median: TimeDelta::seconds(120),
                p90: TimeDelta::seconds(300),
            }),
        };

        assert_eq!(
            summary_row(&summary),
            EvaluationSummaryRowV1 {
                sessions: 2,
                hours: 1.5,
                passes: 3,
                notifications: 4,
                false_alarms: 1,
                repeats: 1,
                misses: 1,
                precision: Some(0.4),
                precision_low: Some(0.4 - 0.1),
                precision_high: Some(0.4 + 0.1),
                recall: Some(0.5),
                recall_low: Some(0.5 - 0.1),
                recall_high: Some(0.5 + 0.1),
                false_alarms_per_hour: None,
                false_alarms_per_hour_low: None,
                false_alarms_per_hour_high: None,
                lead_p10_ms: Some(60_000),
                lead_median_ms: Some(120_000),
                lead_p90_ms: Some(300_000),
            }
        );
    }
}
