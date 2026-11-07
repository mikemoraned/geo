use std::collections::BTreeMap;

use crossings::silver::{Crossing, ReadError, read_of_country};
use domain::{CoordinateError, Precision, Radius, SessionId, Span};
use medallion::{Country, QueryError, Root};
use medallion_model::{
    EvaluationOutcomeRowV1, EvaluationSummaryRowV1, EvaluationTransitionRowV1, TrainSessionRowV3,
};
use predictor::{CrowFlies, Transition};

use crate::oracle::{Oracle, OracleError};
use crate::pick::{Pick, PickError, pick};
use crate::replay::ReplayError;
use crate::rows::{
    RowsError, outcome_rows, replayed_from, scored_from, summary_row, transition_rows,
};
use crate::score::{ScoreError, Scoring};
use crate::store::{compact_ids, compacted, read_passes};
use crate::summarise::{Bootstrap, SummariseError};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Predictor {
    Oracle { horizon: Span },
    CrowFlies { radius: Radius },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatType {
    F32,
    F64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Run {
    pub pick: Pick,
    pub predictor: Predictor,
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
    #[error("a crossing lies at no position: {0}")]
    Position(#[from] CoordinateError),
    #[error(transparent)]
    Oracle(#[from] OracleError),
    #[error(transparent)]
    Replay(#[from] ReplayError),
    #[error(transparent)]
    Score(#[from] ScoreError),
    #[error(transparent)]
    Summarise(#[from] SummariseError),
    #[error(transparent)]
    Rows(#[from] RowsError),
}

pub async fn replay(
    root: &Root,
    country: Country,
    sessions: &[TrainSessionRowV3],
    predictor: Predictor,
    float_type: FloatType,
) -> Result<Vec<EvaluationTransitionRowV1>, StepError> {
    let crossings = read_of_country(root, country).await?;
    let replayed = match float_type {
        FloatType::F32 => replayed::<f32>(root, country, sessions, &crossings, predictor).await?,
        FloatType::F64 => replayed::<f64>(root, country, sessions, &crossings, predictor).await?,
    };
    Ok(transition_rows(&replayed))
}

async fn replayed<F: Precision + Send + Sync>(
    root: &Root,
    country: Country,
    sessions: &[TrainSessionRowV3],
    crossings: &[Crossing],
    predictor: Predictor,
) -> Result<BTreeMap<SessionId, Vec<Transition>>, StepError> {
    let compact_ids = compact_ids(crossings);
    let compact = compacted::<F>(crossings)?;
    Ok(match predictor {
        Predictor::Oracle { horizon } => {
            let oracles = Oracle::per_session(
                sessions.iter().map(|session| session.session_id.clone()),
                read_passes(root, sessions).await?,
                &compact_ids,
                &compact,
                horizon,
            )?;
            crate::replay::replay(root, country, sessions, &compact_ids, |session_id| {
                // Infallible: `per_session` builds an oracle for every session it is given, and
                // the replay asks for one per session of the same list.
                oracles[session_id].clone()
            })
            .await?
        }
        Predictor::CrowFlies { radius } => {
            crate::replay::replay(root, country, sessions, &compact_ids, |_| {
                CrowFlies::new(compact.clone(), radius)
            })
            .await?
        }
    })
}

pub async fn score(
    root: &Root,
    country: Country,
    sessions: &[TrainSessionRowV3],
    transitions: &[EvaluationTransitionRowV1],
    scoring: &Scoring,
) -> Result<Vec<EvaluationOutcomeRowV1>, StepError> {
    let replayed = replayed_from(sessions, transitions)?;
    let crossings = read_of_country(root, country).await?;
    let scored = crate::score::score(
        sessions,
        &replayed,
        read_passes(root, sessions).await?,
        &compact_ids(&crossings),
        scoring,
    )?;
    Ok(outcome_rows(&scored))
}

pub fn summarise(
    sessions: &[TrainSessionRowV3],
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
    let transitions = replay(root, country, &picked, run.predictor, run.float_type).await?;
    let outcomes = score(root, country, &picked, &transitions, &run.scoring).await?;
    let summary = summarise(&picked, &outcomes, &run.bootstrap)?;
    Ok(Ran {
        picked,
        transitions,
        outcomes,
        summary,
    })
}
#[cfg(test)]
mod tests {
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
            &run_of(Predictor::CrowFlies {
                radius: Radius::new(5_000.0).expect("a radius"),
            }),
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

        let transitions = replay(
            &root,
            Country::Germany,
            &ran.picked,
            settings.predictor,
            settings.float_type,
        )
        .await
        .expect("a replay");
        let outcomes = score(
            &root,
            Country::Germany,
            &ran.picked,
            &transitions,
            &settings.scoring,
        )
        .await
        .expect("scores");
        let summary = summarise(&ran.picked, &outcomes, &settings.bootstrap).expect("a summary");

        assert_eq!(
            (transitions, outcomes, summary),
            (ran.transitions, ran.outcomes, ran.summary)
        );
    }
}
