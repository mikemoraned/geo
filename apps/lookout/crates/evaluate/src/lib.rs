use chrono::{NaiveDate, Utc};
use evaluation::PickError;
use evaluation::record::{
    Inputs, RecordError, fingerprints, run_id, run_of, run_row, session_rows,
};
use evaluation::runs::{RunsError, read_run_row, read_run_rows, read_sessions};
use evaluation::steps::{self, Run, StepError};
use medallion::{Root, RunError, RunId, write_run};
use medallion_model::{
    EvaluationOutcomeRowV1, EvaluationSessionRowV1, EvaluationSummaryRowV1,
    EvaluationTransitionRowV1, TRAIN_SESSION_CROSSING_V1, TRAIN_SESSION_SAMPLE_V3,
    TRAIN_SESSION_V3, TrainSessionRowV3, WATER_CROSSING_V2,
};

pub const BUILD_COMMIT: &str = env!("BUILD_GIT_HASH");

const DIRTY: &str = "-dirty";
const UNKNOWN: &str = "unknown";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit(String);

impl Commit {
    pub fn new(hash: &str) -> Result<Self, EvaluateError> {
        if hash.is_empty() || hash == UNKNOWN {
            return Err(EvaluateError::NoCommit);
        }
        if hash.ends_with(DIRTY) {
            return Err(EvaluateError::Uncommitted {
                hash: hash.to_string(),
            });
        }
        Ok(Self(hash.to_string()))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EvaluateError {
    #[error(
        "the code was built with uncommitted changes ({hash}), so a run could not name the code \
         it ran; commit them and build again"
    )]
    Uncommitted { hash: String },
    #[error("the build recorded no commit, so a run could not name the code it ran")]
    NoCommit,
    #[error("run {run_id} was begun by commit {begun}, and this build is commit {current}")]
    CommitChanged {
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

pub async fn pick(root: &Root, run: &Run, commit: &Commit) -> Result<RunId, EvaluateError> {
    let inputs = Inputs {
        code_commit: commit.0.clone(),
        fingerprints: fingerprints(root).await?,
    };
    let row = run_row(run, &inputs, Utc::now());
    let run_id = run_id(&row);
    let date = row.started_at.date_naive();
    let picked = evaluation::pick(root, &run.pick).await?;
    tracing::info!(%run_id, sessions = picked.len(), "picked");
    write_run(root, date, &run_id, &[row]).await?;
    write_run(root, date, &run_id, &session_rows(&picked)).await?;
    Ok(run_id)
}

pub async fn replay(root: &Root, run_id: &RunId, commit: &Commit) -> Result<(), EvaluateError> {
    let begun = resume(root, run_id, commit).await?;
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

pub async fn score(root: &Root, run_id: &RunId, commit: &Commit) -> Result<(), EvaluateError> {
    let begun = resume(root, run_id, commit).await?;
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
    commit: &Commit,
) -> Result<EvaluationSummaryRowV1, EvaluateError> {
    let begun = resume(root, run_id, commit).await?;
    let outcomes = read_run_rows::<EvaluationOutcomeRowV1>(root, run_id).await?;
    let summary = steps::summarise(&begun.picked, &outcomes, &begun.run.bootstrap)?;
    tracing::info!(%run_id, "summarised");
    write_run(root, begun.date, run_id, std::slice::from_ref(&summary)).await?;
    Ok(summary)
}

pub async fn run(
    root: &Root,
    run: &Run,
    commit: &Commit,
) -> Result<(RunId, EvaluationSummaryRowV1), EvaluateError> {
    let run_id = pick(root, run, commit).await?;
    replay(root, &run_id, commit).await?;
    score(root, &run_id, commit).await?;
    let summary = summarise(root, &run_id, commit).await?;
    Ok((run_id, summary))
}

struct Begun {
    run: Run,
    date: NaiveDate,
    picked: Vec<TrainSessionRowV3>,
}

async fn resume(root: &Root, run_id: &RunId, commit: &Commit) -> Result<Begun, EvaluateError> {
    let row = read_run_row(root, run_id).await?;
    if row.code_commit != commit.0 {
        return Err(EvaluateError::CommitChanged {
            run_id: run_id.clone(),
            begun: row.code_commit,
            current: commit.0.clone(),
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
