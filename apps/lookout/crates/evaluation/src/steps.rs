use std::collections::BTreeMap;

use crossings::silver::{Crossing, ReadError, read_of_country};
use domain::{CoordinateError, Precision, SessionId};
use medallion::{Country, QueryError, Root};
use medallion_model::{
    EvaluationOutcomeRowV1, EvaluationSessionRowV1, EvaluationSummaryRowV1,
    EvaluationTransitionRowV1, FloatType, TrainSessionRowV3,
};
use predictor::{Alerting, AlertingBeyondWatching, CrowFlies, Transition, Watching};

use crate::offline::{OfflineError, PassesBySession, passes_by_session};
use crate::oracle::{Oracle, OracleError};
use crate::pick::{Pick, PickError, pick};
use crate::record::session_rows;
use crate::replay::ReplayError;
use crate::rows::{
    RowsError, outcome_rows, replayed_from, scored_from, summary_row, transition_rows,
};
use crate::score::{ScoreError, Scoring};
use crate::settings::{CrowFliesSettings, OnlineDetectorSettings, OracleSettings};
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

#[derive(Debug, Clone, PartialEq)]
pub struct Ran {
    pub picked: Vec<TrainSessionRowV3>,
    pub transitions: Vec<EvaluationTransitionRowV1>,
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
) -> Result<Vec<EvaluationTransitionRowV1>, StepError> {
    let crossings_and_passes = read_crossings_and_passes(root, country, sessions).await?;
    replay_against(
        root,
        country,
        sessions,
        &crossings_and_passes,
        predictor,
        float_type,
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
) -> Result<Vec<EvaluationTransitionRowV1>, StepError> {
    let replayed = match float_type {
        FloatType::F32 => {
            replayed::<f32>(root, country, sessions, crossings_and_passes, predictor).await?
        }
        FloatType::F64 => {
            replayed::<f64>(root, country, sessions, crossings_and_passes, predictor).await?
        }
    };
    Ok(transition_rows(&replayed))
}

async fn replayed<F: Precision + Send + Sync>(
    root: &Root,
    country: Country,
    sessions: &[EvaluationSessionRowV1],
    crossings_and_passes: &CrossingsAndPasses,
    predictor: Predictor,
) -> Result<BTreeMap<SessionId, Vec<Transition>>, StepError> {
    let compact = crossings_and_passes
        .crossings
        .iter()
        .map(crossings::compacted::<F>)
        .collect::<Result<Vec<_>, _>>()?;
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
    let transitions = replay_against(
        root,
        country,
        &sessions,
        &crossings_and_passes,
        run.predictor,
        run.float_type,
    )
    .await?;
    let outcomes = score_against(&sessions, &crossings_and_passes, &transitions, &run.scoring)?;
    let summary = summarise(&sessions, &outcomes, &run.bootstrap)?;
    Ok(Ran {
        picked,
        transitions,
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
        let settings = run_of(oracle());
        let ran = run(&root, &settings).await.expect("a run");
        let sessions = session_rows(&ran.picked);

        let transitions = replay(
            &root,
            Country::Germany,
            &sessions,
            settings.predictor,
            settings.float_type,
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

        assert_eq!(
            (transitions, outcomes, summary),
            (ran.transitions, ran.outcomes, ran.summary)
        );
    }
}
