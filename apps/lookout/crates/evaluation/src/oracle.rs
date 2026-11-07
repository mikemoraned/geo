use std::collections::BTreeMap;

use chrono::{DateTime, TimeDelta, Utc};
use domain::{CrossingCompact, CrossingCompactId, CrossingId, Precision, SessionId, Span};
use geo::{Distance, Haversine};
use geo_types::Point;
use itertools::Itertools;
use predictor::{Confidence, Event, ObserveError, Predict, Prediction, Predictions};

use crate::offline::{OfflineError, RecordedPass, passes_by_session};

#[derive(Debug, Clone, PartialEq)]
pub struct Oracle<P: Precision> {
    passes: Vec<(DateTime<Utc>, CrossingCompactId, Point<P>)>,
    horizon: TimeDelta,
    now: Option<DateTime<Utc>>,
    latest: Option<Point<P>>,
    predictions: Predictions<P>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OracleError {
    #[error(transparent)]
    Passes(#[from] OfflineError),
    #[error(
        "a pass names the crossing {crossing_compact_id}, which the crossings given do not place"
    )]
    Unplaced {
        crossing_compact_id: CrossingCompactId,
    },
}

impl<P: Precision> Oracle<P> {
    pub fn per_session<R: RecordedPass>(
        sessions: impl IntoIterator<Item = SessionId>,
        passes: impl IntoIterator<Item = R>,
        compact_ids: &BTreeMap<CrossingId, CrossingCompactId>,
        crossings: &[CrossingCompact<P>],
        horizon: Span,
    ) -> Result<BTreeMap<SessionId, Self>, OracleError> {
        let positions: BTreeMap<CrossingCompactId, Point<P>> = crossings
            .iter()
            .map(|crossing| (crossing.id, crossing.position))
            .collect();
        passes_by_session(sessions, passes, compact_ids)?
            .into_iter()
            .map(|(session_id, passes)| {
                let placed = passes
                    .into_iter()
                    .map(|(crossed_at, crossing_compact_id)| {
                        positions
                            .get(&crossing_compact_id)
                            .map(|&position| (crossed_at, crossing_compact_id, position))
                            .ok_or(OracleError::Unplaced {
                                crossing_compact_id,
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((session_id, Self::new(placed, horizon)))
            })
            .collect()
    }

    fn new(passes: Vec<(DateTime<Utc>, CrossingCompactId, Point<P>)>, horizon: Span) -> Self {
        Self {
            passes: passes
                .into_iter()
                .sorted_by_key(|&(crossed_at, crossing_compact_id, _)| {
                    (crossed_at, crossing_compact_id)
                })
                .collect(),
            horizon: horizon.time_delta(),
            now: None,
            latest: None,
            predictions: Predictions::new(),
        }
    }

    fn predict(&self, at: DateTime<Utc>, from: Point<P>) -> Predictions<P> {
        let mut predictions = Predictions::new();
        for &(crossed_at, crossing_compact_id, position) in self
            .passes
            .iter()
            .filter(|&&(crossed_at, ..)| at <= crossed_at && crossed_at - at <= self.horizon)
        {
            predictions
                .entry(crossing_compact_id)
                .or_insert_with(|| Prediction {
                    metres: Haversine.distance(from, position),
                    at: Some(crossed_at),
                    confidence: Confidence::High,
                });
        }
        predictions
    }
}

impl<P: Precision> Predict<P> for Oracle<P> {
    fn observe(&mut self, event: Event<P>) -> Result<(), ObserveError> {
        let (at, from) = match event {
            Event::Sampled(sample) => (sample.t, Some(sample.gps.position)),
            Event::Elapsed(at) => (at, self.latest),
        };
        if let Some(now) = self.now
            && at < now
        {
            return Err(ObserveError::OutOfOrder { now, at });
        }
        self.now = Some(at);
        self.latest = from;
        self.predictions = from.map(|from| self.predict(at, from)).unwrap_or_default();
        Ok(())
    }

    fn predictions(&self) -> &Predictions<P> {
        &self.predictions
    }
}

#[cfg(test)]
mod tests {
    use domain::Sample;
    use medallion::Row;
    use medallion_model::TrainSessionCrossingRowV1;
    use predictor::{StateKind, Transition};

    use super::*;
    use crate::offline::OfflineDetector;
    use crate::replay::replay_session;
    use domain::places::DRESDEN_HBF;

    fn instant(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
            + TimeDelta::seconds(seconds)
    }

    fn session() -> SessionId {
        SessionId::new("session-a").expect("a session id")
    }

    fn crossing(n: u32) -> CrossingId {
        CrossingId::new(format!("crossing-{n}")).expect("a crossing id")
    }

    fn compact_ids() -> BTreeMap<CrossingId, CrossingCompactId> {
        (0..10)
            .map(|n| (crossing(n), CrossingCompactId::new(n)))
            .collect()
    }

    fn placed(count: u32) -> Vec<CrossingCompact<f64>> {
        (0..count)
            .map(|n| {
                CrossingCompact::at(n, DRESDEN_HBF.y() + 0.01, DRESDEN_HBF.x()).expect("a crossing")
            })
            .collect()
    }

    fn rows(passes: &[(u32, i64)]) -> Vec<TrainSessionCrossingRowV1> {
        passes
            .iter()
            .map(|&(n, seconds)| TrainSessionCrossingRowV1 {
                session_id: session(),
                crossing_id: crossing(n),
                crossed_at: instant(seconds),
                ..TrainSessionCrossingRowV1::example()
            })
            .collect()
    }

    fn five_minutes() -> Span {
        Span::new("5m").expect("a span")
    }

    fn oracle(passes: &[(u32, i64)]) -> Oracle<f64> {
        Oracle::per_session(
            [session()],
            rows(passes),
            &compact_ids(),
            &placed(10),
            five_minutes(),
        )
        .expect("known sessions and placed crossings")
        .remove(&session())
        .expect("an oracle for the session")
    }

    fn detector(passes: &[(u32, i64)]) -> OfflineDetector {
        OfflineDetector::per_session([session()], rows(passes), &compact_ids())
            .expect("known sessions and crossings")
            .remove(&session())
            .expect("a detector for the session")
    }

    fn sampled(seconds: i64) -> Event<f64> {
        Event::Sampled(
            Sample::at(instant(seconds), DRESDEN_HBF.y(), DRESDEN_HBF.x()).expect("a sample"),
        )
    }

    fn predicted_at(oracle: &mut Oracle<f64>, event: Event<f64>) -> Vec<(u32, Option<i64>)> {
        oracle.observe(event).expect("events in order");
        oracle
            .predictions()
            .iter()
            .map(|(crossing_compact_id, prediction)| {
                assert_eq!(prediction.confidence, Confidence::High);
                (
                    crossing_compact_id.get(),
                    prediction.at.map(|at| (at - instant(0)).num_seconds()),
                )
            })
            .collect()
    }

    #[test]
    fn a_pass_is_predicted_from_the_horizon_before_it_until_its_own_sample() {
        let mut oracle = oracle(&[(1, 400)]);

        let predicting: Vec<i64> = (0..=10)
            .map(|step| step * 50)
            .filter(|&seconds| !predicted_at(&mut oracle, sampled(seconds)).is_empty())
            .collect();

        assert_eq!(predicting, [100, 150, 200, 250, 300, 350, 400]);
    }

    #[test]
    fn a_prediction_holds_the_time_of_its_pass_and_the_distance_from_the_sample() {
        let mut oracle = oracle(&[(1, 60)]);

        oracle.observe(sampled(0)).expect("a sample");
        let prediction = oracle.predictions()[&CrossingCompactId::new(1)];

        assert_eq!(prediction.at, Some(instant(60)));
        assert!(
            (prediction.metres - 1_111.95).abs() < 0.1,
            "{}",
            prediction.metres
        );
    }

    #[test]
    fn of_two_passes_of_one_crossing_the_next_is_predicted() {
        let mut oracle = oracle(&[(1, 250), (1, 100)]);

        assert_eq!(predicted_at(&mut oracle, sampled(50)), [(1, Some(100))]);
        assert_eq!(predicted_at(&mut oracle, sampled(120)), [(1, Some(250))]);
    }

    #[test]
    fn time_passing_without_a_sample_brings_a_pass_within_the_horizon() {
        let mut oracle = oracle(&[(1, 400)]);

        predicted_at(&mut oracle, sampled(50));

        assert_eq!(
            predicted_at(&mut oracle, Event::Elapsed(instant(120))),
            [(1, Some(400))]
        );
    }

    #[test]
    fn a_session_with_no_passes_predicts_nothing() {
        let mut oracle = oracle(&[]);

        assert!(predicted_at(&mut oracle, sampled(0)).is_empty());
    }

    #[test]
    fn an_event_behind_the_last_is_refused() {
        let mut oracle = oracle(&[(1, 400)]);
        predicted_at(&mut oracle, sampled(10));

        assert_eq!(
            oracle.observe(sampled(9)),
            Err(ObserveError::OutOfOrder {
                now: instant(10),
                at: instant(9)
            })
        );
    }

    #[test]
    fn a_pass_of_a_crossing_the_crossings_do_not_place_is_refused() {
        let refused = Oracle::per_session(
            [session()],
            rows(&[(7, 400)]),
            &compact_ids(),
            &placed(5),
            five_minutes(),
        );

        assert_eq!(
            refused,
            Err(OracleError::Unplaced {
                crossing_compact_id: CrossingCompactId::new(7)
            })
        );
    }

    #[test]
    fn replayed_the_oracle_alerts_the_horizon_before_each_pass_and_each_alert_comes_true() {
        let samples = (0..=60).map(|step| {
            Sample::at(instant(step * 10), DRESDEN_HBF.y(), DRESDEN_HBF.x()).expect("a sample")
        });

        let transitions: Vec<(i64, StateKind, StateKind)> =
            replay_session(samples, oracle(&[(1, 400)]), detector(&[(1, 400)]))
                .expect("a replay")
                .iter()
                .map(|transition: &Transition| {
                    (
                        (transition.at - instant(0)).num_seconds(),
                        transition.from,
                        transition.to,
                    )
                })
                .collect();

        assert_eq!(
            transitions,
            [
                (100, StateKind::OutOfReach, StateKind::AlertedPass),
                (400, StateKind::AlertedPass, StateKind::ActualPass),
                (410, StateKind::ActualPass, StateKind::OutOfReach),
            ]
        );
    }
}
