use std::collections::BTreeMap;

use datafusion::prelude::col;
use domain::{CoordinateError, CrossingCompactId, CrossingId, Precision, Sample, SessionId};
use medallion::{Country, Query, QueryError, Root};
use medallion_model::{TrainSessionRowV3, TrainSessionSampleRowV3};
use predictor::{DetectPasses, Event, ObserveError, Predict, TrackError, Tracker, Transition};
use rayon::prelude::*;

use crate::offline::{OfflineDetector, OfflineError};
use crate::store::{picked, read_passes};

#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error("reading the sessions' samples and passes: {0}")]
    Query(#[from] QueryError),
    #[error("gathering the sessions' passes: {0}")]
    Passes(#[from] OfflineError),
    #[error("a sample of {session_id} lies at no position: {source}")]
    Position {
        session_id: SessionId,
        source: CoordinateError,
    },
    #[error("the predictor refused a sample: {0}")]
    Predicting(ObserveError),
    #[error("the pass detector refused a sample: {0}")]
    Detecting(ObserveError),
    #[error("the tracker refused a round: {0}")]
    Tracking(#[from] TrackError),
}

pub fn replay_session<F, Pr, D>(
    samples: impl IntoIterator<Item = Sample<F>>,
    mut predictor: Pr,
    mut detector: D,
) -> Result<Vec<Transition>, ReplayError>
where
    F: Precision,
    Pr: Predict<F>,
    D: DetectPasses<F>,
{
    let mut tracker = Tracker::new();
    let mut transitions = Vec::new();
    for sample in samples {
        predictor
            .observe(Event::Sampled(sample))
            .map_err(ReplayError::Predicting)?;
        detector
            .observe(Event::Sampled(sample))
            .map_err(ReplayError::Detecting)?;
        transitions.extend(tracker.track(sample.t, predictor.predictions(), detector.passed())?);
    }
    Ok(transitions)
}

pub async fn replay<F, Pr>(
    root: &Root,
    country: Country,
    sessions: &[TrainSessionRowV3],
    compact_ids: &BTreeMap<CrossingId, CrossingCompactId>,
    predictor: impl Fn(&SessionId) -> Pr + Sync,
) -> Result<BTreeMap<SessionId, Vec<Transition>>, ReplayError>
where
    F: Precision + Send,
    Pr: Predict<F> + Send,
{
    let sample_rows = Query::new(root.clone())
        .read_of_country::<TrainSessionSampleRowV3>(country)
        .await?
        .filter(picked(sessions))?
        .sort(vec![
            col("session_id").sort(true, false),
            col("t").sort(true, false),
            col("seq").sort(true, false),
        ])?
        .rows()
        .await?;
    let passes = read_passes(root, sessions).await?;
    let detectors = OfflineDetector::per_session(
        sessions.iter().map(|session| session.session_id.clone()),
        passes,
        compact_ids,
    )?;

    let mut samples_by_session: BTreeMap<SessionId, Vec<Sample<F>>> = BTreeMap::new();
    for row in sample_rows {
        let sample =
            Sample::at(row.t, row.lat, row.lon).map_err(|source| ReplayError::Position {
                session_id: row.session_id.clone(),
                source,
            })?;
        samples_by_session
            .entry(row.session_id)
            .or_default()
            .push(sample);
    }
    let sessions: Vec<(SessionId, Vec<Sample<F>>, OfflineDetector)> = detectors
        .into_iter()
        .map(|(session_id, detector)| {
            let samples = samples_by_session.remove(&session_id).unwrap_or_default();
            (session_id, samples, detector)
        })
        .collect();
    sessions
        .into_par_iter()
        .map(|(session_id, samples, detector)| {
            let transitions = replay_session(samples, predictor(&session_id), detector)?;
            Ok((session_id, transitions))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{DateTime, TimeDelta, Utc};
    use domain::{CrossingCompact, CrossingCompactId, CrossingId, Radius, SessionId};
    use geo_types::Point;
    use medallion::{GeographyRow, Row, write_geography_rows, write_rows};
    use medallion_model::{TrainSessionCrossingRowV1, TrainSessionSampleRowV3};
    use predictor::{Confidence, CrowFlies, Prediction, Predictions, StateKind};

    use super::*;
    use crate::offline::OfflineDetector;

    fn instant(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
            + TimeDelta::seconds(seconds)
    }

    fn session() -> SessionId {
        named("a")
    }

    fn named(name: &str) -> SessionId {
        SessionId::new(format!("session-{name}")).expect("a session id")
    }

    fn crossing(n: u32) -> CrossingId {
        CrossingId::new(format!("crossing-{n}")).expect("a crossing id")
    }

    fn detector(passes: &[(u32, i64)]) -> OfflineDetector {
        let rows = passes
            .iter()
            .map(|&(n, seconds)| TrainSessionCrossingRowV1 {
                session_id: session(),
                crossing_id: crossing(n),
                crossed_at: instant(seconds),
                ..TrainSessionCrossingRowV1::example()
            });
        let compact_ids = (0..10)
            .map(|n| (crossing(n), CrossingCompactId::new(n)))
            .collect();
        OfflineDetector::per_session([session()], rows, &compact_ids)
            .expect("known sessions and crossings")
            .remove(&session())
            .expect("a detector for the session")
    }

    fn sample<F: Precision>(seconds: i64, latitude: f64) -> Sample<F> {
        Sample::at(instant(seconds), latitude, 13.73).expect("a sample")
    }

    struct Scripted {
        script: BTreeMap<DateTime<Utc>, Predictions<f64>>,
        current: Predictions<f64>,
    }

    impl Scripted {
        fn alerting(crossing: u32, seconds: &[i64]) -> Self {
            let alert = Prediction {
                metres: 100.0,
                at: None,
                confidence: Confidence::High,
            };
            Self {
                script: seconds
                    .iter()
                    .map(|&at| (instant(at), Predictions::from([(crossing.into(), alert)])))
                    .collect(),
                current: Predictions::new(),
            }
        }
    }

    impl Predict<f64> for Scripted {
        fn observe(&mut self, event: Event<f64>) -> Result<(), ObserveError> {
            let Event::Sampled(sample) = event else {
                return Ok(());
            };
            self.current = self.script.get(&sample.t).cloned().unwrap_or_default();
            Ok(())
        }

        fn predictions(&self) -> &Predictions<f64> {
            &self.current
        }
    }

    fn kinds(transitions: &[Transition]) -> Vec<(i64, u32, StateKind, StateKind)> {
        transitions
            .iter()
            .map(|transition| {
                (
                    (transition.at - instant(0)).num_seconds(),
                    transition.crossing_compact_id.get(),
                    transition.from,
                    transition.to,
                )
            })
            .collect()
    }

    #[test]
    fn an_alert_then_its_pass_records_each_state_at_the_sample_it_starts() {
        let samples = [0, 10, 20, 30].map(|seconds| sample(seconds, 51.0));

        let transitions = replay_session(
            samples,
            Scripted::alerting(1, &[0, 10, 20]),
            detector(&[(1, 15)]),
        )
        .expect("a replay");

        assert_eq!(
            kinds(&transitions),
            [
                (0, 1, StateKind::OutOfReach, StateKind::AlertedPass),
                (20, 1, StateKind::AlertedPass, StateKind::ActualPass),
                (30, 1, StateKind::ActualPass, StateKind::OutOfReach),
            ]
        );
    }

    #[test]
    fn crow_flies_in_f32_takes_a_crossing_in_and_out_of_its_radius_as_a_train_passes_it() {
        let crossings: Vec<CrossingCompact<f32>> =
            vec![CrossingCompact::at(1, 51.05, 13.73).expect("a crossing")];
        let predictor = CrowFlies::new(crossings, Radius::new(1_000.0).expect("a radius"));
        let samples = (0..50).map(|step| sample::<f32>(step * 10, 51.0 + 0.002 * step as f64));

        let transitions =
            replay_session(samples, predictor, detector(&[(1, 250)])).expect("a replay");

        assert_eq!(
            kinds(&transitions)
                .into_iter()
                .map(|(_, crossing, from, to)| (crossing, from, to))
                .collect::<Vec<_>>(),
            [
                (1, StateKind::OutOfReach, StateKind::CandidatePass),
                (1, StateKind::CandidatePass, StateKind::OutOfReach),
            ]
        );
    }

    #[test]
    fn a_sample_behind_the_last_is_refused() {
        let crossings: Vec<CrossingCompact<f64>> =
            vec![CrossingCompact::at(1, 51.05, 13.73).expect("a crossing")];
        let predictor = CrowFlies::new(crossings, Radius::new(1_000.0).expect("a radius"));
        let samples = [sample(10, 51.0), sample(0, 51.0)];

        let refused = replay_session(samples, predictor, detector(&[]));

        assert!(
            matches!(
                refused,
                Err(ReplayError::Predicting(ObserveError::OutOfOrder { now, at }))
                    if now == instant(10) && at == instant(0)
            ),
            "{refused:?}"
        );
    }

    fn compact_ids() -> BTreeMap<CrossingId, CrossingCompactId> {
        (0..10)
            .map(|n| (crossing(n), CrossingCompactId::new(n)))
            .collect()
    }

    fn stored_sample(
        name: &str,
        seq: u32,
        seconds: i64,
    ) -> GeographyRow<TrainSessionSampleRowV3, Point<f64>> {
        let latitude = 51.0 + 0.002 * f64::from(seq);
        GeographyRow {
            row: TrainSessionSampleRowV3 {
                session_id: named(name),
                t: instant(seconds),
                seq,
                lat: latitude,
                lon: 13.73,
                ..TrainSessionSampleRowV3::example()
            },
            geography: Point::new(13.73, latitude),
            country: Country::Germany,
        }
    }

    fn stored_pass(name: &str, n: u32, seconds: i64) -> TrainSessionCrossingRowV1 {
        TrainSessionCrossingRowV1 {
            session_id: named(name),
            crossing_id: crossing(n),
            crossed_at: instant(seconds),
            ..TrainSessionCrossingRowV1::example()
        }
    }

    fn picked(names: &[&str]) -> Vec<TrainSessionRowV3> {
        names
            .iter()
            .map(|&name| TrainSessionRowV3 {
                session_id: named(name),
                ..TrainSessionRowV3::example()
            })
            .collect()
    }

    async fn stored(
        dir: &std::path::Path,
        samples: &[GeographyRow<TrainSessionSampleRowV3, Point<f64>>],
        passes: &[TrainSessionCrossingRowV1],
    ) -> Root {
        let root = Root::new(dir);
        write_geography_rows(&root, samples)
            .await
            .expect("write the samples");
        write_rows(&root, passes).await.expect("write the passes");
        root
    }

    fn every_session_samples(
        names: &[&str],
    ) -> Vec<GeographyRow<TrainSessionSampleRowV3, Point<f64>>> {
        names
            .iter()
            .flat_map(|&name| (0..4).map(move |seq| stored_sample(name, seq, i64::from(seq) * 10)))
            .collect()
    }

    #[tokio::test]
    async fn each_session_picked_is_replayed_against_its_own_passes_and_no_other_session_is() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(
            tmp.path(),
            &every_session_samples(&["a", "b", "c"]),
            &[
                stored_pass("a", 1, 15),
                stored_pass("b", 1, 5),
                stored_pass("c", 1, 25),
            ],
        )
        .await;

        let replayed = replay(
            &root,
            Country::Germany,
            &picked(&["a", "c"]),
            &compact_ids(),
            |_| Scripted::alerting(1, &[0, 10, 20, 30]),
        )
        .await
        .expect("a replay");

        let actual_at: BTreeMap<SessionId, Vec<i64>> = replayed
            .iter()
            .map(|(session_id, transitions)| {
                (
                    session_id.clone(),
                    kinds(transitions)
                        .into_iter()
                        .filter(|&(_, _, _, to)| to == StateKind::ActualPass)
                        .map(|(seconds, ..)| seconds)
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            actual_at,
            BTreeMap::from([(named("a"), vec![20]), (named("c"), vec![30])])
        );
    }

    #[tokio::test]
    async fn a_session_is_replayed_in_order_of_time_however_its_samples_were_written() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let mut samples = every_session_samples(&["a"]);
        samples.reverse();
        let root = stored(tmp.path(), &samples, &[stored_pass("a", 1, 15)]).await;

        let replayed = replay(
            &root,
            Country::Germany,
            &picked(&["a"]),
            &compact_ids(),
            |_| Scripted::alerting(1, &[0, 10, 20, 30]),
        )
        .await
        .expect("a replay in order");

        assert_eq!(
            kinds(&replayed[&named("a")]),
            [
                (0, 1, StateKind::OutOfReach, StateKind::AlertedPass),
                (20, 1, StateKind::AlertedPass, StateKind::ActualPass),
                (30, 1, StateKind::ActualPass, StateKind::AlertedPass),
            ]
        );
    }

    #[tokio::test]
    async fn a_session_picked_with_no_samples_is_replayed_as_no_transitions() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(
            tmp.path(),
            &every_session_samples(&["a"]),
            &[stored_pass("a", 1, 15)],
        )
        .await;

        let replayed = replay(
            &root,
            Country::Germany,
            &picked(&["a", "z"]),
            &compact_ids(),
            |_| Scripted::alerting(1, &[0, 10, 20, 30]),
        )
        .await
        .expect("a replay");

        assert_eq!(replayed.get(&named("z")), Some(&Vec::new()));
    }
}
