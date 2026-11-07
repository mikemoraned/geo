use std::collections::BTreeMap;
use std::iter::Sum;
use std::num::NonZeroUsize;
use std::ops::Add;

use chrono::TimeDelta;
use domain::SessionId;
use medallion_model::TrainSessionRowV3;

use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;
use statrs::statistics::{Data, OrderStatistics};

use crate::pick::Seed;
use crate::score::{Outcome, Verdict};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bootstrap {
    pub resamples: NonZeroUsize,
    pub seed: Seed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    pub value: f64,
    pub low: f64,
    pub high: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeadTimes {
    pub p10: TimeDelta,
    pub median: TimeDelta,
    pub p90: TimeDelta,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub sessions: usize,
    pub hours: f64,
    pub passes: usize,
    pub notifications: usize,
    pub false_alarms: usize,
    pub repeats: usize,
    pub misses: usize,
    pub precision: Option<Estimate>,
    pub recall: Option<Estimate>,
    pub false_alarms_per_hour: Option<Estimate>,
    pub lead_times: Option<LeadTimes>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SummariseError {
    #[error("the session {session_id} was picked, and the scores hold no outcomes for it")]
    NotScored { session_id: SessionId },
}

pub fn summarise(
    sessions: &[TrainSessionRowV3],
    scored: &BTreeMap<SessionId, Vec<Outcome>>,
    bootstrap: &Bootstrap,
) -> Result<Summary, SummariseError> {
    let tallies: Vec<Tally> = sessions
        .iter()
        .map(|session| {
            let outcomes =
                scored
                    .get(&session.session_id)
                    .ok_or_else(|| SummariseError::NotScored {
                        session_id: session.session_id.clone(),
                    })?;
            let hours = (session.ended_at - session.started_at).as_seconds_f64() / 3_600.0;
            Ok(Tally::of(outcomes, hours))
        })
        .collect::<Result<_, SummariseError>>()?;
    let whole: Tally = tallies.iter().copied().sum();
    let resamples = resampled(&tallies, bootstrap);
    let leads: Vec<f64> = sessions
        .iter()
        .filter_map(|session| scored.get(&session.session_id))
        .flatten()
        .filter_map(|outcome| match outcome {
            Outcome::Notification {
                verdict: Verdict::Credited { credit, .. },
                ..
            } => Some(credit.lead.as_seconds_f64()),
            _ => None,
        })
        .collect();

    Ok(Summary {
        sessions: sessions.len(),
        hours: whole.hours,
        passes: whole.passes,
        notifications: whole.notifications,
        false_alarms: whole.false_alarms,
        repeats: whole.repeats,
        misses: whole.misses,
        precision: estimate(&whole, &resamples, Tally::precision),
        recall: estimate(&whole, &resamples, Tally::recall),
        false_alarms_per_hour: estimate(&whole, &resamples, Tally::false_alarms_per_hour),
        lead_times: lead_times(leads),
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Tally {
    credited_score: f64,
    notifications: usize,
    passes: usize,
    false_alarms: usize,
    repeats: usize,
    misses: usize,
    hours: f64,
}

impl Tally {
    fn of(outcomes: &[Outcome], hours: f64) -> Self {
        outcomes.iter().fold(
            Self {
                hours,
                ..Self::default()
            },
            |tally, outcome| match outcome {
                Outcome::Notification { verdict, .. } => match verdict {
                    Verdict::Credited { score, .. } => Self {
                        credited_score: tally.credited_score + score.get(),
                        notifications: tally.notifications + 1,
                        passes: tally.passes + 1,
                        ..tally
                    },
                    Verdict::Repeat(_) => Self {
                        notifications: tally.notifications + 1,
                        repeats: tally.repeats + 1,
                        ..tally
                    },
                    Verdict::FalseAlarm => Self {
                        notifications: tally.notifications + 1,
                        false_alarms: tally.false_alarms + 1,
                        ..tally
                    },
                    Verdict::Unscored(_) => tally,
                },
                Outcome::Miss { .. } => Self {
                    passes: tally.passes + 1,
                    misses: tally.misses + 1,
                    ..tally
                },
            },
        )
    }

    fn precision(&self) -> Option<f64> {
        (self.notifications > 0).then(|| self.credited_score / self.notifications as f64)
    }

    fn recall(&self) -> Option<f64> {
        (self.passes > 0).then(|| self.credited_score / self.passes as f64)
    }

    fn false_alarms_per_hour(&self) -> Option<f64> {
        (self.hours > 0.0).then(|| self.false_alarms as f64 / self.hours)
    }
}

impl Add for Tally {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            credited_score: self.credited_score + other.credited_score,
            notifications: self.notifications + other.notifications,
            passes: self.passes + other.passes,
            false_alarms: self.false_alarms + other.false_alarms,
            repeats: self.repeats + other.repeats,
            misses: self.misses + other.misses,
            hours: self.hours + other.hours,
        }
    }
}

impl Sum for Tally {
    fn sum<I: Iterator<Item = Self>>(tallies: I) -> Self {
        tallies.fold(Self::default(), Add::add)
    }
}

const INTERVAL: (f64, f64) = (0.025, 0.975);

fn resampled(tallies: &[Tally], bootstrap: &Bootstrap) -> Vec<Tally> {
    if tallies.is_empty() {
        return Vec::new();
    }
    let mut rng = ChaCha8Rng::seed_from_u64(bootstrap.seed.0);
    (0..bootstrap.resamples.get())
        .map(|_| {
            (0..tallies.len())
                .map(|_| tallies[rng.random_range(0..tallies.len())])
                .sum()
        })
        .collect()
}

fn estimate(
    whole: &Tally,
    resamples: &[Tally],
    measure: fn(&Tally) -> Option<f64>,
) -> Option<Estimate> {
    let value = measure(whole)?;
    let values: Vec<f64> = resamples.iter().filter_map(measure).collect();
    if values.is_empty() {
        return None;
    }
    let mut data = Data::new(values);
    Some(Estimate {
        value,
        low: data.quantile(INTERVAL.0),
        high: data.quantile(INTERVAL.1),
    })
}

fn lead_times(leads: Vec<f64>) -> Option<LeadTimes> {
    if leads.is_empty() {
        return None;
    }
    let mut data = Data::new(leads);
    let at = |seconds: f64| TimeDelta::milliseconds((seconds * 1_000.0).round() as i64);
    Some(LeadTimes {
        p10: at(data.quantile(0.1)),
        median: at(data.quantile(0.5)),
        p90: at(data.quantile(0.9)),
    })
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use domain::{CrossingCompact, CrossingCompactId, CrossingId, Sample, Span};
    use itertools::Itertools;
    use medallion::Row;
    use medallion_model::TrainSessionCrossingRowV1;
    use proptest::prelude::*;

    use super::*;
    use crate::offline::{OfflineDetector, passes_by_session};
    use crate::oracle::Oracle;
    use crate::replay::replay_session;
    use crate::score::{Class, Credit, Score, Scoring, score};
    use domain::places::DRESDEN_HBF;

    fn instant(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
            + TimeDelta::seconds(seconds)
    }

    fn session(name: &str) -> SessionId {
        SessionId::new(format!("session-{name}")).expect("a session id")
    }

    fn lasting(name: &str, seconds: i64) -> TrainSessionRowV3 {
        TrainSessionRowV3 {
            session_id: session(name),
            started_at: instant(0),
            ended_at: instant(seconds),
            ..TrainSessionRowV3::example()
        }
    }

    fn bootstrap(seed: u64) -> Bootstrap {
        Bootstrap {
            resamples: NonZeroUsize::new(1_000).expect("not zero"),
            seed: Seed(seed),
        }
    }

    fn credit(class: Class, lead: i64) -> Credit {
        Credit {
            crossed_at: instant(1_000),
            lead: TimeDelta::seconds(lead),
            class,
        }
    }

    fn credited(class: Class, score: f64, lead: i64) -> Verdict {
        Verdict::Credited {
            credit: credit(class, lead),
            score: Score::new(score).expect("a score"),
        }
    }

    fn notified(verdict: Verdict) -> Outcome {
        Outcome::Notification {
            crossing_compact_id: CrossingCompactId::new(1),
            notified_at: instant(100),
            verdict,
        }
    }

    fn missed() -> Outcome {
        Outcome::Miss {
            crossing_compact_id: CrossingCompactId::new(1),
            crossed_at: instant(500),
        }
    }

    fn hit() -> Outcome {
        notified(credited(Class::Hit, 1.0, 120))
    }

    fn false_alarm() -> Outcome {
        notified(Verdict::FalseAlarm)
    }

    fn exact(value: f64) -> Option<Estimate> {
        Some(Estimate {
            value,
            low: value,
            high: value,
        })
    }

    #[test]
    fn a_session_is_summed_into_counts_and_measures_with_unscored_outcomes_left_out() {
        let scored = BTreeMap::from([(
            session("a"),
            vec![
                hit(),
                notified(credited(Class::TooEarly, 0.5, 330)),
                notified(Verdict::Repeat(credit(Class::Hit, 60))),
                false_alarm(),
                notified(Verdict::Unscored(credit(Class::Hit, 60))),
                missed(),
            ],
        )]);

        let summary = summarise(&[lasting("a", 3_600)], &scored, &bootstrap(1)).expect("a summary");

        assert_eq!(
            summary,
            Summary {
                sessions: 1,
                hours: 1.0,
                passes: 3,
                notifications: 4,
                false_alarms: 1,
                repeats: 1,
                misses: 1,
                precision: exact(0.375),
                recall: exact(0.5),
                false_alarms_per_hour: exact(1.0),
                lead_times: Some(LeadTimes {
                    p10: TimeDelta::seconds(120),
                    median: TimeDelta::seconds(225),
                    p90: TimeDelta::seconds(330),
                }),
            }
        );
    }

    #[test]
    fn a_measure_with_nothing_to_measure_is_absent() {
        let scored = BTreeMap::from([(session("a"), vec![])]);

        let summary = summarise(&[lasting("a", 3_600)], &scored, &bootstrap(1)).expect("a summary");

        assert_eq!(
            (summary.precision, summary.recall, summary.lead_times),
            (None, None, None)
        );
        assert_eq!(summary.false_alarms_per_hour, exact(0.0));
    }

    #[test]
    fn an_interval_spans_the_values_a_resample_of_the_sessions_can_take() {
        let scored = BTreeMap::from([
            (session("a"), vec![hit()]),
            (session("b"), vec![false_alarm()]),
        ]);

        let summary = summarise(
            &[lasting("a", 3_600), lasting("b", 3_600)],
            &scored,
            &bootstrap(1),
        )
        .expect("a summary");

        assert_eq!(
            summary.precision,
            Some(Estimate {
                value: 0.5,
                low: 0.0,
                high: 1.0
            })
        );
    }

    #[test]
    fn one_seed_gives_the_same_intervals_on_every_run() {
        let scored: BTreeMap<SessionId, Vec<Outcome>> = (0..20)
            .map(|n| {
                let outcomes = if n % 3 == 0 {
                    vec![false_alarm()]
                } else {
                    vec![hit()]
                };
                (session(&n.to_string()), outcomes)
            })
            .collect();
        let sessions: Vec<TrainSessionRowV3> =
            (0..20).map(|n| lasting(&n.to_string(), 3_600)).collect();

        let once = summarise(&sessions, &scored, &bootstrap(7)).expect("a summary");
        let again = summarise(&sessions, &scored, &bootstrap(7)).expect("a summary");

        assert_eq!(once, again);
    }

    #[test]
    fn a_session_picked_and_not_scored_is_refused() {
        let refused = summarise(&[lasting("a", 3_600)], &BTreeMap::new(), &bootstrap(1));

        assert_eq!(
            refused,
            Err(SummariseError::NotScored {
                session_id: session("a")
            })
        );
    }

    fn crossing(n: u32) -> CrossingId {
        CrossingId::new(format!("crossing-{n}")).expect("a crossing id")
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

    fn oracle_summary(sessions: &[(i64, Vec<(u32, i64)>)]) -> Summary {
        let picked: Vec<TrainSessionRowV3> = sessions
            .iter()
            .enumerate()
            .map(|(n, (seconds, _))| lasting(&n.to_string(), *seconds))
            .collect();
        let passes: Vec<TrainSessionCrossingRowV1> = sessions
            .iter()
            .enumerate()
            .flat_map(|(n, (_, passes))| {
                passes
                    .iter()
                    .map(move |&(crossing_n, seconds)| TrainSessionCrossingRowV1 {
                        session_id: session(&n.to_string()),
                        crossing_id: crossing(crossing_n),
                        crossed_at: instant(seconds),
                        ..TrainSessionCrossingRowV1::example()
                    })
            })
            .collect();
        let compact_ids: BTreeMap<CrossingId, CrossingCompactId> = (0..10)
            .map(|n| (crossing(n), CrossingCompactId::new(n)))
            .collect();
        let crossings: Vec<CrossingCompact<f64>> = (0..10)
            .map(|n| {
                CrossingCompact::at(n, DRESDEN_HBF.y() + 0.01, DRESDEN_HBF.x()).expect("a crossing")
            })
            .collect();
        let resolved = passes_by_session(
            picked.iter().map(|session| session.session_id.clone()),
            passes,
            &compact_ids,
        )
        .expect("known sessions and crossings");
        let mut oracles =
            Oracle::per_session(&resolved, &crossings, first_runs().horizon).expect("oracles");
        let mut detectors = OfflineDetector::per_session(&resolved);

        let replayed: BTreeMap<SessionId, Vec<predictor::Transition>> = picked
            .iter()
            .map(|session| {
                let samples =
                    (0..=(session.ended_at - session.started_at).num_seconds() / 10).map(|step| {
                        Sample::<f64>::at(instant(step * 10), DRESDEN_HBF.y(), DRESDEN_HBF.x())
                            .expect("a sample")
                    });
                let transitions = replay_session(
                    samples,
                    oracles.remove(&session.session_id).expect("an oracle"),
                    detectors.remove(&session.session_id).expect("a detector"),
                )
                .expect("a replay");
                (session.session_id.clone(), transitions)
            })
            .collect();
        let scored = score(&picked, &replayed, &resolved, &first_runs()).expect("scores");
        summarise(&picked, &scored, &bootstrap(1)).expect("a summary")
    }

    fn passes_of_crossings() -> impl Strategy<Value = Vec<(u32, i64)>> {
        prop::collection::vec(
            (0u32..5, 0i64..60, prop::collection::vec(4i64..80, 0..4)),
            0..6,
        )
        .prop_map(|crossings| {
            crossings
                .into_iter()
                .unique_by(|&(crossing, ..)| crossing)
                .flat_map(|(crossing, first, gaps)| {
                    gaps.into_iter()
                        .scan(first, |at, gap| {
                            *at += gap;
                            Some(*at)
                        })
                        .chain([first])
                        .map(move |steps| (crossing, steps * 10))
                        .collect::<Vec<_>>()
                })
                .collect()
        })
    }

    proptest! {

        #[test]
        fn the_oracle_scores_perfectly_when_no_two_passes_of_a_crossing_fall_within_40_s(
            sessions in prop::collection::vec(passes_of_crossings(), 1..4),
        ) {
            let sessions: Vec<(i64, Vec<(u32, i64)>)> = sessions
                .into_iter()
                .map(|passes| (3_600, passes))
                .collect();

            let summary = oracle_summary(&sessions);

            prop_assert_eq!(summary.false_alarms, 0);
            prop_assert_eq!(summary.repeats, 0);
            prop_assert_eq!(summary.misses, 0);
            prop_assert_eq!(summary.false_alarms_per_hour, exact(0.0));
            if summary.passes > 0 {
                prop_assert_eq!(summary.precision, exact(1.0));
                prop_assert_eq!(summary.recall, exact(1.0));
            }
        }
    }

    #[test]
    fn the_oracle_alerts_again_for_a_pass_after_one_at_the_first_sample() {
        let summary = oracle_summary(&[(3_600, vec![(0, 0), (0, 60)])]);

        assert_eq!((summary.passes, summary.misses), (1, 0));
        assert_eq!(summary.recall, exact(1.0));
    }

    #[test]
    fn the_oracle_falls_short_on_a_crossing_passed_twice_20_s_apart() {
        let summary = oracle_summary(&[(3_600, vec![(1, 300), (1, 320)])]);

        assert_eq!((summary.passes, summary.notifications), (2, 2));
        assert_eq!(summary.recall, exact(0.625));
        assert_eq!(summary.precision, exact(0.625));
    }
}
