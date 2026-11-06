use std::collections::BTreeMap;

use chrono::{DateTime, TimeDelta, Utc};
use domain::{CrossingCompactId, CrossingId, SessionId, Span};
use itertools::Itertools;
use medallion_model::TrainSessionRowV3;
use predictor::{StateKind, Transition};

use crate::offline::{OfflineError, RecordedPass, SessionPasses, passes_by_session};

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Score(f64);

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[error("a score lies from 0 to 1, and {0} does not")]
pub struct NotAScore(f64);

impl Score {
    pub fn new(value: f64) -> Result<Self, NotAScore> {
        (0.0..=1.0)
            .contains(&value)
            .then_some(Self(value))
            .ok_or(NotAScore(value))
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scoring {
    pub horizon: Span,
    pub last_moment: Span,
    pub ramp: Span,
    pub last_moment_score: Score,
    pub floor: Score,
    pub warm_up: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    Hit,
    LastMoment,
    TooEarly,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Credit {
    pub crossed_at: DateTime<Utc>,
    pub lead: TimeDelta,
    pub class: Class,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    Credited { credit: Credit, score: Score },
    Repeat(Credit),
    FalseAlarm,
    Unscored(Credit),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    Notification {
        crossing_compact_id: CrossingCompactId,
        notified_at: DateTime<Utc>,
        verdict: Verdict,
    },
    Miss {
        crossing_compact_id: CrossingCompactId,
        crossed_at: DateTime<Utc>,
    },
}

impl Scoring {
    fn credit(&self, notified_at: DateTime<Utc>, crossed_at: DateTime<Utc>) -> (Credit, Score) {
        let lead = crossed_at - notified_at;
        let (class, score) = if lead < self.last_moment.time_delta() {
            (Class::LastMoment, self.last_moment_score)
        } else if lead <= self.horizon.time_delta() {
            (Class::Hit, Score(1.0))
        } else {
            let past = (lead - self.horizon.time_delta()).as_seconds_f64()
                / self.ramp.time_delta().as_seconds_f64();
            let ramped = 1.0 - past * (1.0 - self.floor.get());
            (Class::TooEarly, Score(ramped.max(self.floor.get())))
        };
        (
            Credit {
                crossed_at,
                lead,
                class,
            },
            score,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScoreError {
    #[error(transparent)]
    Passes(#[from] OfflineError),
    #[error("the session {session_id} was picked, and the replay holds no transitions for it")]
    NotReplayed { session_id: SessionId },
}

pub fn score<R: RecordedPass>(
    sessions: &[TrainSessionRowV3],
    replayed: &BTreeMap<SessionId, Vec<Transition>>,
    passes: impl IntoIterator<Item = R>,
    compact_ids: &BTreeMap<CrossingId, CrossingCompactId>,
    scoring: &Scoring,
) -> Result<BTreeMap<SessionId, Vec<Outcome>>, ScoreError> {
    let mut passes_by_session = passes_by_session(
        sessions.iter().map(|session| session.session_id.clone()),
        passes,
        compact_ids,
    )?;
    sessions
        .iter()
        .map(|session| {
            let transitions =
                replayed
                    .get(&session.session_id)
                    .ok_or_else(|| ScoreError::NotReplayed {
                        session_id: session.session_id.clone(),
                    })?;
            let passes = passes_by_session
                .remove(&session.session_id)
                .unwrap_or_default();
            let scored_from = session.started_at + scoring.warm_up.time_delta();
            Ok((
                session.session_id.clone(),
                score_session(transitions, &passes, scored_from, scoring),
            ))
        })
        .collect()
}

type CrossingAt = (DateTime<Utc>, CrossingCompactId);

fn score_session(
    transitions: &[Transition],
    passes: &SessionPasses,
    scored_from: DateTime<Utc>,
    scoring: &Scoring,
) -> Vec<Outcome> {
    let notifications: Vec<CrossingAt> = transitions
        .iter()
        .filter(|transition| transition.to == StateKind::AlertedPass)
        .map(|transition| (transition.at, transition.crossing_compact_id))
        .collect();
    let candidates: BTreeMap<usize, (CrossingAt, Credit, Score)> = notifications
        .iter()
        .enumerate()
        .filter_map(|(index, &(notified_at, crossing_compact_id))| {
            passes
                .iter()
                .filter(|&&(crossed_at, passed)| {
                    passed == crossing_compact_id && crossed_at >= notified_at
                })
                .min()
                .map(|&pass| {
                    let (credit, score) = scoring.credit(notified_at, pass.0);
                    (index, (pass, credit, score))
                })
        })
        .collect();

    let mut credited: BTreeMap<CrossingAt, (usize, Score)> = BTreeMap::new();
    for (&index, &(pass, _, score)) in &candidates {
        credited
            .entry(pass)
            .and_modify(|best| {
                if score < best.1 {
                    *best = (index, score);
                }
            })
            .or_insert((index, score));
    }

    let verdict_of = |index: usize| match candidates.get(&index) {
        None => Verdict::FalseAlarm,
        Some((pass, credit, _)) if pass.0 < scored_from => Verdict::Unscored(*credit),
        Some((pass, credit, score)) if credited.get(pass).is_some_and(|best| best.0 == index) => {
            Verdict::Credited {
                credit: *credit,
                score: *score,
            }
        }
        Some((_, credit, _)) => Verdict::Repeat(*credit),
    };
    let notified =
        notifications
            .iter()
            .enumerate()
            .map(
                |(index, &(notified_at, crossing_compact_id))| Outcome::Notification {
                    crossing_compact_id,
                    notified_at,
                    verdict: verdict_of(index),
                },
            );
    let missed = passes
        .iter()
        .filter(|pass| pass.0 >= scored_from && !credited.contains_key(pass))
        .map(|&(crossed_at, crossing_compact_id)| Outcome::Miss {
            crossing_compact_id,
            crossed_at,
        });
    notified
        .chain(missed)
        .sorted_by_key(|outcome| match *outcome {
            Outcome::Notification {
                notified_at,
                crossing_compact_id,
                ..
            } => (notified_at, crossing_compact_id),
            Outcome::Miss {
                crossed_at,
                crossing_compact_id,
            } => (crossed_at, crossing_compact_id),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use medallion::Row;
    use medallion_model::TrainSessionCrossingRowV1;
    use proptest::prelude::*;

    use super::*;

    fn instant(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
            + TimeDelta::seconds(seconds)
    }

    fn alerted(crossing: u32, seconds: i64) -> Transition {
        Transition {
            crossing_compact_id: CrossingCompactId::new(crossing),
            at: instant(seconds),
            from: StateKind::CandidatePass,
            to: StateKind::AlertedPass,
        }
    }

    fn withdrawn(crossing: u32, seconds: i64) -> Transition {
        Transition {
            crossing_compact_id: CrossingCompactId::new(crossing),
            at: instant(seconds),
            from: StateKind::AlertedPass,
            to: StateKind::CandidatePass,
        }
    }

    fn passed(passes: &[(u32, i64)]) -> SessionPasses {
        passes
            .iter()
            .map(|&(crossing, seconds)| (instant(seconds), CrossingCompactId::new(crossing)))
            .collect()
    }

    fn first_runs() -> Scoring {
        Scoring {
            horizon: Span::new("5m").expect("a span"),
            last_moment: Span::new("30s").expect("a span"),
            ramp: Span::new("1m").expect("a span"),
            last_moment_score: Score::new(0.25).expect("a score"),
            floor: Score::new(0.05).expect("a score"),
            warm_up: Span::new("1m").expect("a span"),
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Seen {
        Credited(Class, f64),
        Repeat,
        FalseAlarm,
        Unscored,
    }

    fn long_ago() -> DateTime<Utc> {
        instant(-3_600)
    }

    type Scored = (i64, Seen);

    fn notifications(outcomes: &[Outcome]) -> Vec<Scored> {
        outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                Outcome::Notification {
                    notified_at,
                    verdict,
                    ..
                } => Some(((*notified_at - instant(0)).num_seconds(), seen(*verdict))),
                Outcome::Miss { .. } => None,
            })
            .collect()
    }

    fn seen(verdict: Verdict) -> Seen {
        match verdict {
            Verdict::Credited { credit, score } => {
                Seen::Credited(credit.class, (score.get() * 1_000.0).round() / 1_000.0)
            }
            Verdict::Repeat(_) => Seen::Repeat,
            Verdict::FalseAlarm => Seen::FalseAlarm,
            Verdict::Unscored(_) => Seen::Unscored,
        }
    }

    fn misses(outcomes: &[Outcome]) -> Vec<(u32, i64)> {
        outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                Outcome::Miss {
                    crossing_compact_id,
                    crossed_at,
                } => Some((
                    crossing_compact_id.get(),
                    (*crossed_at - instant(0)).num_seconds(),
                )),
                Outcome::Notification { .. } => None,
            })
            .collect()
    }

    fn scored_alone(lead: i64) -> Vec<Scored> {
        notifications(&score_session(
            &[alerted(1, 1_000 - lead)],
            &passed(&[(1, 1_000)]),
            long_ago(),
            &first_runs(),
        ))
    }

    #[test]
    fn a_notification_from_30_s_to_the_horizon_ahead_is_a_hit() {
        assert_eq!(scored_alone(300), [(700, Seen::Credited(Class::Hit, 1.0))]);
        assert_eq!(scored_alone(30), [(970, Seen::Credited(Class::Hit, 1.0))]);
    }

    #[test]
    fn a_notification_under_30_s_ahead_is_last_moment() {
        assert_eq!(
            scored_alone(29),
            [(971, Seen::Credited(Class::LastMoment, 0.25))]
        );
        assert_eq!(
            scored_alone(0),
            [(1_000, Seen::Credited(Class::LastMoment, 0.25))]
        );
    }

    #[test]
    fn a_notification_past_the_horizon_scores_down_the_ramp_to_the_floor() {
        assert_eq!(
            scored_alone(330),
            [(670, Seen::Credited(Class::TooEarly, 0.525))]
        );
        assert_eq!(
            scored_alone(360),
            [(640, Seen::Credited(Class::TooEarly, 0.05))]
        );
        assert_eq!(
            scored_alone(900),
            [(100, Seen::Credited(Class::TooEarly, 0.05))]
        );
    }

    #[test]
    fn a_notification_with_no_pass_of_its_crossing_to_come_is_a_false_alarm() {
        let outcomes = score_session(
            &[alerted(1, 100), alerted(2, 100)],
            &passed(&[(1, 50), (3, 200)]),
            long_ago(),
            &first_runs(),
        );

        assert_eq!(
            notifications(&outcomes),
            [(100, Seen::FalseAlarm), (100, Seen::FalseAlarm)]
        );
    }

    #[test]
    fn a_pass_that_credits_no_notification_is_a_miss() {
        let outcomes = score_session(&[], &passed(&[(1, 50), (2, 80)]), long_ago(), &first_runs());

        assert_eq!(misses(&outcomes), [(1, 50), (2, 80)]);
    }

    #[test]
    fn a_pass_credits_the_notification_that_scores_worst_and_the_others_are_repeats() {
        let outcomes = score_session(
            &[
                alerted(1, 0),
                withdrawn(1, 50),
                alerted(1, 750),
                withdrawn(1, 800),
                alerted(1, 990),
            ],
            &passed(&[(1, 1_000)]),
            long_ago(),
            &first_runs(),
        );

        assert_eq!(
            notifications(&outcomes),
            [
                (0, Seen::Credited(Class::TooEarly, 0.05)),
                (750, Seen::Repeat),
                (990, Seen::Repeat)
            ]
        );
        assert!(misses(&outcomes).is_empty());
    }

    #[test]
    fn of_notifications_scoring_alike_the_earliest_earns_the_credit() {
        let outcomes = score_session(
            &[alerted(1, 750), withdrawn(1, 800), alerted(1, 900)],
            &passed(&[(1, 1_000)]),
            long_ago(),
            &first_runs(),
        );

        assert_eq!(
            notifications(&outcomes),
            [(750, Seen::Credited(Class::Hit, 1.0)), (900, Seen::Repeat)]
        );
    }

    #[test]
    fn a_notification_is_measured_to_the_next_pass_of_its_crossing() {
        let outcomes = score_session(
            &[alerted(1, 400)],
            &passed(&[(1, 300), (1, 600), (1, 900)]),
            long_ago(),
            &first_runs(),
        );

        assert_eq!(
            notifications(&outcomes),
            [(400, Seen::Credited(Class::Hit, 1.0))]
        );
        assert_eq!(misses(&outcomes), [(1, 300), (1, 900)]);
    }

    #[test]
    fn a_score_outside_0_to_1_is_refused() {
        assert_eq!(Score::new(1.01), Err(NotAScore(1.01)));
        assert_eq!(Score::new(-0.01), Err(NotAScore(-0.01)));
        assert!(Score::new(f64::NAN).is_err());
    }

    fn session(name: &str) -> SessionId {
        SessionId::new(format!("session-{name}")).expect("a session id")
    }

    fn crossing(n: u32) -> CrossingId {
        CrossingId::new(format!("crossing-{n}")).expect("a crossing id")
    }

    fn row(name: &str, n: u32, seconds: i64) -> TrainSessionCrossingRowV1 {
        TrainSessionCrossingRowV1 {
            session_id: session(name),
            crossing_id: crossing(n),
            crossed_at: instant(seconds),
            ..TrainSessionCrossingRowV1::example()
        }
    }

    fn compact_ids() -> BTreeMap<CrossingId, CrossingCompactId> {
        (0..10)
            .map(|n| (crossing(n), CrossingCompactId::new(n)))
            .collect()
    }

    #[test]
    fn each_session_is_scored_against_its_own_passes() {
        let replayed = BTreeMap::from([
            (session("a"), vec![alerted(1, 700)]),
            (session("b"), vec![]),
        ]);

        let scored = score(
            &started(&[("a", -3_600), ("b", -3_600)]),
            &replayed,
            [row("a", 1, 1_000), row("b", 1, 1_000)],
            &compact_ids(),
            &first_runs(),
        )
        .expect("known sessions and crossings");

        assert_eq!(
            notifications(&scored[&session("a")]),
            [(700, Seen::Credited(Class::Hit, 1.0))]
        );
        assert!(misses(&scored[&session("a")]).is_empty());
        assert_eq!(misses(&scored[&session("b")]), [(1, 1_000)]);
    }

    fn started(sessions: &[(&str, i64)]) -> Vec<TrainSessionRowV3> {
        sessions
            .iter()
            .map(|&(name, seconds)| TrainSessionRowV3 {
                session_id: session(name),
                started_at: instant(seconds),
                ..TrainSessionRowV3::example()
            })
            .collect()
    }

    fn from_start(transitions: &[Transition], passes: &[(u32, i64)]) -> Vec<Outcome> {
        score(
            &started(&[("a", 0)]),
            &BTreeMap::from([(session("a"), transitions.to_vec())]),
            passes.iter().map(|&(n, seconds)| row("a", n, seconds)),
            &compact_ids(),
            &first_runs(),
        )
        .expect("known sessions and crossings")
        .remove(&session("a"))
        .expect("the session's outcomes")
    }

    #[test]
    fn a_pass_in_a_sessions_first_minute_is_neither_scored_nor_missed() {
        let outcomes = from_start(&[], &[(1, 59), (2, 60)]);

        assert_eq!(misses(&outcomes), [(2, 60)]);
    }

    #[test]
    fn a_notification_whose_pass_falls_in_the_first_minute_is_unscored_with_its_repeats() {
        let outcomes = from_start(
            &[alerted(1, 10), withdrawn(1, 20), alerted(1, 30)],
            &[(1, 50)],
        );

        assert_eq!(
            notifications(&outcomes),
            [(10, Seen::Unscored), (30, Seen::Unscored)]
        );
    }

    #[test]
    fn a_notification_in_the_first_minute_is_scored_against_a_later_pass() {
        let outcomes = from_start(&[alerted(1, 30), alerted(2, 40)], &[(1, 300)]);

        assert_eq!(
            notifications(&outcomes),
            [
                (30, Seen::Credited(Class::Hit, 1.0)),
                (40, Seen::FalseAlarm)
            ]
        );
    }

    #[test]
    fn a_session_picked_and_not_replayed_is_refused() {
        let refused = score(
            &started(&[("a", 0), ("b", 0)]),
            &BTreeMap::from([(session("a"), vec![])]),
            Vec::<TrainSessionCrossingRowV1>::new(),
            &compact_ids(),
            &first_runs(),
        );

        assert_eq!(
            refused,
            Err(ScoreError::NotReplayed {
                session_id: session("b")
            })
        );
    }

    proptest! {
        #[test]
        fn each_pass_credits_one_notification_or_is_missed_and_each_notification_is_scored_once(
            alerts in prop::collection::vec((0u32..3, 0i64..1_200), 0..20),
            passes in prop::collection::vec((0u32..3, 0i64..1_200), 0..20),
        ) {
            let transitions: Vec<Transition> = alerts
                .iter()
                .sorted_by_key(|&&(_, seconds)| seconds)
                .map(|&(crossing, seconds)| alerted(crossing, seconds))
                .collect();
            let passes = passed(&passes.into_iter().unique().collect::<Vec<_>>());

            let outcomes = score_session(&transitions, &passes, long_ago(), &first_runs());

            let credited = notifications(&outcomes).iter().filter(|(_, seen)| matches!(seen, Seen::Credited(..))).count();
            prop_assert_eq!(notifications(&outcomes).len(), transitions.len());
            prop_assert_eq!(credited + misses(&outcomes).len(), passes.len());
        }
    }
}
