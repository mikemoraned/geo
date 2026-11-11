use std::collections::BTreeMap;

use crossings::silver::{Crossing, ReadError, read_of_country};
use domain::{CoordinateError, Precision, SessionId};
use medallion::{Country, QueryError, Root};
use medallion_model::{
    EvaluationOutcomeRowV1, EvaluationSessionRowV1, EvaluationSkewRowV1, EvaluationSummaryRowV1,
    EvaluationTransitionRowV1, FloatType, TrainSessionRowV3,
};
use predictor::{Alerting, AlertingBeyondWatching, CrowFlies, OnlineDetector, Watching};

use crate::offline::{OfflineError, PassesBySession, passes_by_session};
use crate::oracle::{Oracle, OracleError};
use crate::pick::{Pick, PickError, pick};
use crate::record::session_rows;
use crate::replay::{ReplayError, SessionReplay};
use crate::rows::{
    RowsError, outcome_rows, replayed_from, scored_from, skew_row, summary_row, transition_rows,
};
use crate::score::{ScoreError, Scoring};
use crate::settings::{CrowFliesSettings, OnlineDetectorSettings, OracleSettings};
use crate::skew::{SkewError, Visit, skew};
use crate::store::{compact_ids, read_passes};
use crate::summarise::{Bootstrap, SummariseError};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Predictor {
    Oracle(OracleSettings),
    CrowFlies(CrowFliesSettings),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Run {
    pub pick: Pick,
    pub predictor: Predictor,
    pub online_detector: Option<OnlineDetectorSettings>,
    pub float_type: FloatType,
    pub scoring: Scoring,
    pub bootstrap: Bootstrap,
}

impl Run {
    pub fn skewing(&self) -> Option<Skewing> {
        self.online_detector.map(|online_detector| Skewing {
            online_detector,
            bootstrap: self.bootstrap,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Skewing {
    pub online_detector: OnlineDetectorSettings,
    pub bootstrap: Bootstrap,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Replayed {
    pub transitions: Vec<EvaluationTransitionRowV1>,
    pub skew: Option<EvaluationSkewRowV1>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ran {
    pub picked: Vec<TrainSessionRowV3>,
    pub transitions: Vec<EvaluationTransitionRowV1>,
    pub skew: Option<EvaluationSkewRowV1>,
    pub outcomes: Vec<EvaluationOutcomeRowV1>,
    pub summary: EvaluationSummaryRowV1,
}

#[derive(Debug, thiserror::Error)]
pub enum StepError {
    #[error(transparent)]
    Pick(#[from] PickError),
    #[error("reading the store: {0}")]
    Query(#[from] QueryError),
    #[error("reading the crossings: {0}")]
    Crossings(#[from] ReadError),
    #[error("gathering the sessions' passes: {0}")]
    Passes(#[from] OfflineError),
    #[error("a crossing lies at no position: {0}")]
    Position(#[from] CoordinateError),
    #[error(transparent)]
    Oracle(#[from] OracleError),
    #[error(transparent)]
    CrowFlies(#[from] AlertingBeyondWatching),
    #[error(transparent)]
    Replay(#[from] ReplayError),
    #[error(transparent)]
    Skew(#[from] SkewError),
    #[error(transparent)]
    Score(#[from] ScoreError),
    #[error(transparent)]
    Summarise(#[from] SummariseError),
    #[error(transparent)]
    Rows(#[from] RowsError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CrossingsAndPasses {
    pub crossings: Vec<Crossing>,
    pub passes: PassesBySession,
}

pub async fn read_crossings_and_passes(
    root: &Root,
    country: Country,
    sessions: &[EvaluationSessionRowV1],
) -> Result<CrossingsAndPasses, StepError> {
    let crossings = read_of_country(root, country).await?;
    let passes = passes_by_session(
        sessions.iter().map(|session| session.session_id.clone()),
        read_passes(root, sessions).await?,
        &compact_ids(&crossings),
    )?;
    Ok(CrossingsAndPasses { crossings, passes })
}

pub async fn replay(
    root: &Root,
    country: Country,
    sessions: &[EvaluationSessionRowV1],
    predictor: Predictor,
    float_type: FloatType,
    skewing: Option<Skewing>,
) -> Result<Replayed, StepError> {
    let crossings_and_passes = read_crossings_and_passes(root, country, sessions).await?;
    replay_against(
        root,
        country,
        sessions,
        &crossings_and_passes,
        predictor,
        float_type,
        skewing,
    )
    .await
}

pub async fn replay_against(
    root: &Root,
    country: Country,
    sessions: &[EvaluationSessionRowV1],
    crossings_and_passes: &CrossingsAndPasses,
    predictor: Predictor,
    float_type: FloatType,
    skewing: Option<Skewing>,
) -> Result<Replayed, StepError> {
    let online_detector = skewing.map(|skewing| skewing.online_detector);
    let replayed = match float_type {
        FloatType::F32 => {
            replayed::<f32>(
                root,
                country,
                sessions,
                crossings_and_passes,
                predictor,
                online_detector,
            )
            .await?
        }
        FloatType::F64 => {
            replayed::<f64>(
                root,
                country,
                sessions,
                crossings_and_passes,
                predictor,
                online_detector,
            )
            .await?
        }
    };
    let (transitions, visits): (BTreeMap<_, _>, BTreeMap<_, _>) = replayed
        .into_iter()
        .map(
            |(
                session_id,
                SessionReplay {
                    transitions,
                    visits,
                },
            )| { ((session_id.clone(), transitions), (session_id, visits)) },
        )
        .unzip();
    let skew = skewing
        .map(|skewing| {
            let visits: BTreeMap<SessionId, Vec<Visit>> = visits
                .into_iter()
                .filter_map(|(session_id, visits)| Some((session_id, visits?)))
                .collect();
            skew(
                sessions,
                &crossings_and_passes.passes,
                &visits,
                &skewing.bootstrap,
            )
        })
        .transpose()?;
    Ok(Replayed {
        transitions: transition_rows(&transitions),
        skew: skew.as_ref().map(skew_row),
    })
}

async fn replayed<F: Precision + Send + Sync>(
    root: &Root,
    country: Country,
    sessions: &[EvaluationSessionRowV1],
    crossings_and_passes: &CrossingsAndPasses,
    predictor: Predictor,
    online_detector: Option<OnlineDetectorSettings>,
) -> Result<BTreeMap<SessionId, SessionReplay>, StepError> {
    let compact = crossings_and_passes
        .crossings
        .iter()
        .map(crossings::compacted::<F>)
        .collect::<Result<Vec<_>, _>>()?;
    let online_detector =
        online_detector.map(|settings| OnlineDetector::new(compact.clone(), settings.pass_radius));
    Ok(match predictor {
        Predictor::Oracle(OracleSettings { horizon }) => {
            let oracles = Oracle::per_session(&crossings_and_passes.passes, &compact, horizon)?;
            crate::replay::replay(
                root,
                country,
                sessions,
                &crossings_and_passes.passes,
                |session_id| {
                    // Infallible: `per_session` builds an oracle for every session it is given, and
                    // the replay asks for one per session of the same list.
                    oracles[session_id].clone()
                },
                online_detector.as_ref(),
            )
            .await?
        }
        Predictor::CrowFlies(CrowFliesSettings { radius, horizon }) => {
            let crow_flies =
                CrowFlies::new(compact, Watching { radius }, Alerting { radius, horizon })?;
            crate::replay::replay(
                root,
                country,
                sessions,
                &crossings_and_passes.passes,
                |_| crow_flies.clone(),
                online_detector.as_ref(),
            )
            .await?
        }
    })
}

pub async fn score(
    root: &Root,
    country: Country,
    sessions: &[EvaluationSessionRowV1],
    transitions: &[EvaluationTransitionRowV1],
    scoring: &Scoring,
) -> Result<Vec<EvaluationOutcomeRowV1>, StepError> {
    let crossings_and_passes = read_crossings_and_passes(root, country, sessions).await?;
    score_against(sessions, &crossings_and_passes, transitions, scoring)
}

pub fn score_against(
    sessions: &[EvaluationSessionRowV1],
    crossings_and_passes: &CrossingsAndPasses,
    transitions: &[EvaluationTransitionRowV1],
    scoring: &Scoring,
) -> Result<Vec<EvaluationOutcomeRowV1>, StepError> {
    let replayed = replayed_from(sessions, transitions)?;
    let scored = crate::score::score(sessions, &replayed, &crossings_and_passes.passes, scoring)?;
    Ok(outcome_rows(&scored))
}

pub fn summarise(
    sessions: &[EvaluationSessionRowV1],
    outcomes: &[EvaluationOutcomeRowV1],
    bootstrap: &Bootstrap,
) -> Result<EvaluationSummaryRowV1, StepError> {
    let scored = scored_from(sessions, outcomes)?;
    Ok(summary_row(&crate::summarise::summarise(
        sessions, &scored, bootstrap,
    )?))
}

pub async fn run(root: &Root, run: &Run) -> Result<Ran, StepError> {
    let country = run.pick.country;
    let picked = pick(root, &run.pick).await?;
    let sessions = session_rows(&picked);
    let crossings_and_passes = read_crossings_and_passes(root, country, &sessions).await?;
    let Replayed { transitions, skew } = replay_against(
        root,
        country,
        &sessions,
        &crossings_and_passes,
        run.predictor,
        run.float_type,
        run.skewing(),
    )
    .await?;
    let outcomes = score_against(&sessions, &crossings_and_passes, &transitions, &run.scoring)?;
    let summary = summarise(&sessions, &outcomes, &run.bootstrap)?;
    Ok(Ran {
        picked,
        transitions,
        skew,
        outcomes,
        summary,
    })
}
#[cfg(test)]
mod tests {
    use domain::{Radius, Span};

    use super::*;
    use crate::fixtures::{oracle, run_of, store};

    #[tokio::test]
    async fn the_oracle_run_end_to_end_scores_perfectly() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = store(tmp.path(), 3).await;

        let ran = run(&root, &run_of(oracle())).await.expect("a run");

        assert_eq!(ran.picked.len(), 3);
        assert_eq!(
            (
                ran.summary.sessions,
                ran.summary.passes,
                ran.summary.notifications,
                ran.summary.false_alarms,
                ran.summary.misses
            ),
            (3, 3, 3, 0, 0)
        );
        assert_eq!(
            (
                ran.summary.precision,
                ran.summary.precision_low,
                ran.summary.recall,
                ran.summary.recall_low
            ),
            (Some(1.0), Some(1.0), Some(1.0), Some(1.0))
        );
    }

    #[tokio::test]
    async fn crow_flies_run_end_to_end_notifies_nothing_and_misses_every_pass() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = store(tmp.path(), 3).await;

        let ran = run(
            &root,
            &run_of(Predictor::CrowFlies(CrowFliesSettings {
                radius: Radius::new(5_000.0).expect("a radius"),
                horizon: Span::new("5m").expect("a span"),
            })),
        )
        .await
        .expect("a run");

        assert!(!ran.transitions.is_empty());
        assert_eq!(
            (
                ran.summary.notifications,
                ran.summary.misses,
                ran.summary.recall
            ),
            (0, 3, Some(0.0))
        );
    }

    #[tokio::test]
    async fn the_steps_run_one_by_one_give_what_the_run_gives() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = store(tmp.path(), 3).await;
        let settings = Run {
            online_detector: Some(OnlineDetectorSettings {
                pass_radius: Radius::new(100.0).expect("a radius"),
            }),
            ..run_of(oracle())
        };
        let ran = run(&root, &settings).await.expect("a run");
        let sessions = session_rows(&ran.picked);

        let Replayed { transitions, skew } = replay(
            &root,
            Country::Germany,
            &sessions,
            settings.predictor,
            settings.float_type,
            settings.skewing(),
        )
        .await
        .expect("a replay");
        let outcomes = score(
            &root,
            Country::Germany,
            &sessions,
            &transitions,
            &settings.scoring,
        )
        .await
        .expect("scores");
        let summary = summarise(&sessions, &outcomes, &settings.bootstrap).expect("a summary");

        assert!(
            ran.skew.is_some(),
            "a run with an online detector measures the skew"
        );
        assert_eq!(
            (transitions, skew, outcomes, summary),
            (ran.transitions, ran.skew, ran.outcomes, ran.summary)
        );
    }
}
