use chrono::NaiveDate;
use evaluation::PickError;
use evaluation::record::{CodeVersion, RecordError, fingerprints, record, run_of};
use evaluation::runs::{RunsError, read_run_row, read_run_rows, read_sessions};
use evaluation::steps::{self, Run, StepError};
use medallion::{Root, RunError, RunId, write_run};
use medallion_model::{
    EvaluationOutcomeRowV1, EvaluationSessionRowV1, EvaluationSummaryRowV1,
    EvaluationTransitionRowV1, TRAIN_SESSION_CROSSING_V1, TRAIN_SESSION_SAMPLE_V3,
    TRAIN_SESSION_V3, TrainSessionRowV3, WATER_CROSSING_V2,
};

pub const BUILD_CODE_VERSION: &str = env!("BUILD_CODE_VERSION");

#[derive(Debug, thiserror::Error)]
pub enum EvaluateError {
    #[error("run {run_id} was begun by code {begun}, and this build is code {current}")]
    CodeChanged {
        run_id: RunId,
        begun: String,
        current: String,
    },
    #[error("silver has changed since run {run_id} was begun, in {datasets:?}")]
    DataChanged {
        run_id: RunId,
        datasets: Vec<&'static str>,
    },
    #[error(transparent)]
    Pick(#[from] PickError),
    #[error(transparent)]
    Record(#[from] RecordError),
    #[error(transparent)]
    Runs(#[from] RunsError),
    #[error(transparent)]
    Step(#[from] StepError),
    #[error("writing the run to gold: {0}")]
    Write(#[from] RunError),
}

pub async fn pick(root: &Root, run: &Run, code: &CodeVersion) -> Result<RunId, EvaluateError> {
    let picked = evaluation::pick(root, &run.pick).await?;
    let recorded = record(root, run, code, &picked).await?;
    tracing::info!(run_id = %recorded.run_id, sessions = picked.len(), "picked");
    write_run(root, recorded.date, &recorded.run_id, &[recorded.row]).await?;
    write_run(root, recorded.date, &recorded.run_id, &recorded.sessions).await?;
    Ok(recorded.run_id)
}

pub async fn replay(root: &Root, run_id: &RunId, code: &CodeVersion) -> Result<(), EvaluateError> {
    let begun = resume(root, run_id, code).await?;
    let transitions = steps::replay(
        root,
        begun.run.pick.country,
        &begun.picked,
        begun.run.predictor,
        begun.run.float_type,
    )
    .await?;
    tracing::info!(%run_id, transitions = transitions.len(), "replayed");
    write_run(root, begun.date, run_id, &transitions).await?;
    Ok(())
}

pub async fn score(root: &Root, run_id: &RunId, code: &CodeVersion) -> Result<(), EvaluateError> {
    let begun = resume(root, run_id, code).await?;
    let transitions = read_run_rows::<EvaluationTransitionRowV1>(root, run_id).await?;
    let outcomes = steps::score(
        root,
        begun.run.pick.country,
        &begun.picked,
        &transitions,
        &begun.run.scoring,
    )
    .await?;
    tracing::info!(%run_id, outcomes = outcomes.len(), "scored");
    write_run(root, begun.date, run_id, &outcomes).await?;
    Ok(())
}

pub async fn summarise(
    root: &Root,
    run_id: &RunId,
    code: &CodeVersion,
) -> Result<EvaluationSummaryRowV1, EvaluateError> {
    let begun = resume(root, run_id, code).await?;
    let outcomes = read_run_rows::<EvaluationOutcomeRowV1>(root, run_id).await?;
    let summary = steps::summarise(&begun.picked, &outcomes, &begun.run.bootstrap)?;
    tracing::info!(%run_id, "summarised");
    write_run(root, begun.date, run_id, std::slice::from_ref(&summary)).await?;
    Ok(summary)
}

pub async fn run(
    root: &Root,
    run: &Run,
    code: &CodeVersion,
) -> Result<(RunId, EvaluationSummaryRowV1), EvaluateError> {
    let run_id = pick(root, run, code).await?;
    replay(root, &run_id, code).await?;
    score(root, &run_id, code).await?;
    let summary = summarise(root, &run_id, code).await?;
    Ok((run_id, summary))
}

struct Begun {
    run: Run,
    date: NaiveDate,
    picked: Vec<TrainSessionRowV3>,
}

async fn resume(root: &Root, run_id: &RunId, code: &CodeVersion) -> Result<Begun, EvaluateError> {
    let row = read_run_row(root, run_id).await?;
    if row.code_version != code.as_str() {
        return Err(EvaluateError::CodeChanged {
            run_id: run_id.clone(),
            begun: row.code_version,
            current: code.as_str().to_string(),
        });
    }
    let now = fingerprints(root).await?;
    let changed: Vec<&'static str> = [
        (
            TRAIN_SESSION_V3.name,
            &row.train_session_v3_fingerprint,
            &now.train_session_v3,
        ),
        (
            TRAIN_SESSION_SAMPLE_V3.name,
            &row.train_session_sample_v3_fingerprint,
            &now.train_session_sample_v3,
        ),
        (
            TRAIN_SESSION_CROSSING_V1.name,
            &row.train_session_crossing_v1_fingerprint,
            &now.train_session_crossing_v1,
        ),
        (
            WATER_CROSSING_V2.name,
            &row.water_crossing_v2_fingerprint,
            &now.water_crossing_v2,
        ),
    ]
    .into_iter()
    .filter(|(_, begun, current)| begun != current)
    .map(|(dataset, _, _)| dataset)
    .collect();
    if !changed.is_empty() {
        return Err(EvaluateError::DataChanged {
            run_id: run_id.clone(),
            datasets: changed,
        });
    }
    let run = run_of(&row)?;
    let sessions = read_run_rows::<EvaluationSessionRowV1>(root, run_id).await?;
    let picked = read_sessions(root, run.pick.country, &sessions).await?;
    Ok(Begun {
        run,
        date: row.started_at.date_naive(),
        picked,
    })
}
