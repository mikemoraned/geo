use chrono::NaiveDate;
use medallion::{Root, RunError, RunId, write_run};
use medallion_model::{
    EvaluationOutcomeRowV1, EvaluationSessionRowV1, EvaluationSummaryRowV1,
    EvaluationTransitionRowV1, TrainSessionRowV3,
};

use crate::PickError;
use crate::fingerprints::Fingerprints;
use code_version::CodeVersion;

use crate::record::{RecordError, Recorded, record};
use crate::runs::{RunsError, read_run_row, read_run_rows};
use crate::steps::{self, Ran, Run, StepError};

#[derive(Debug, thiserror::Error)]
pub enum GoldError {
    #[error("run {run_id} was begun by code {begun}, and this build is code {current}")]
    CodeChanged {
        run_id: RunId,
        begun: String,
        current: String,
    },
    #[error("silver has changed since run {run_id} was begun, in {datasets:?}")]
    DataChanged {
        run_id: RunId,
        datasets: Vec<String>,
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

pub async fn pick(root: &Root, run: &Run, code: &CodeVersion) -> Result<RunId, GoldError> {
    let picked = crate::pick(root, &run.pick).await?;
    Ok(begin(root, run, code, &picked).await?.run_id)
}

pub async fn replay(root: &Root, run_id: &RunId, code: &CodeVersion) -> Result<(), GoldError> {
    let begun = resume(root, run_id, code).await?;
    let transitions = steps::replay(
        root,
        begun.run.pick.country,
        &begun.sessions,
        begun.run.predictor,
        begun.run.float_type,
    )
    .await?;
    tracing::info!(%run_id, transitions = transitions.len(), "replayed");
    write_run(root, begun.date, run_id, &transitions).await?;
    Ok(())
}

pub async fn score(root: &Root, run_id: &RunId, code: &CodeVersion) -> Result<(), GoldError> {
    let begun = resume(root, run_id, code).await?;
    let transitions = read_run_rows::<EvaluationTransitionRowV1>(root, run_id).await?;
    let outcomes = steps::score(
        root,
        begun.run.pick.country,
        &begun.sessions,
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
) -> Result<EvaluationSummaryRowV1, GoldError> {
    let begun = resume(root, run_id, code).await?;
    let outcomes = read_run_rows::<EvaluationOutcomeRowV1>(root, run_id).await?;
    let summary = steps::summarise(&begun.sessions, &outcomes, &begun.run.bootstrap)?;
    tracing::info!(%run_id, "summarised");
    write_run(root, begun.date, run_id, std::slice::from_ref(&summary)).await?;
    Ok(summary)
}

pub async fn run(root: &Root, run: &Run, code: &CodeVersion) -> Result<(RunId, Ran), GoldError> {
    let country = run.pick.country;
    let picked = crate::pick(root, &run.pick).await?;
    let Recorded {
        run_id,
        date,
        sessions,
        ..
    } = begin(root, run, code, &picked).await?;

    let crossings_and_passes = steps::read_crossings_and_passes(root, country, &sessions).await?;
    let transitions = steps::replay_against(
        root,
        country,
        &sessions,
        &crossings_and_passes,
        run.predictor,
        run.float_type,
    )
    .await?;
    tracing::info!(%run_id, transitions = transitions.len(), "replayed");
    write_run(root, date, &run_id, &transitions).await?;

    let outcomes =
        steps::score_against(&sessions, &crossings_and_passes, &transitions, &run.scoring)?;
    tracing::info!(%run_id, outcomes = outcomes.len(), "scored");
    write_run(root, date, &run_id, &outcomes).await?;

    let summary = steps::summarise(&sessions, &outcomes, &run.bootstrap)?;
    tracing::info!(%run_id, "summarised");
    write_run(root, date, &run_id, std::slice::from_ref(&summary)).await?;

    Ok((
        run_id,
        Ran {
            picked,
            transitions,
            outcomes,
            summary,
        },
    ))
}

async fn begin(
    root: &Root,
    run: &Run,
    code: &CodeVersion,
    picked: &[TrainSessionRowV3],
) -> Result<Recorded, GoldError> {
    let recorded = record(root, run, code, picked).await?;
    tracing::info!(run_id = %recorded.run_id, sessions = picked.len(), "picked");
    write_run(
        root,
        recorded.date,
        &recorded.run_id,
        std::slice::from_ref(&recorded.row),
    )
    .await?;
    write_run(root, recorded.date, &recorded.run_id, &recorded.sessions).await?;
    Ok(recorded)
}

struct Begun {
    run: Run,
    date: NaiveDate,
    sessions: Vec<EvaluationSessionRowV1>,
}

async fn resume(root: &Root, run_id: &RunId, code: &CodeVersion) -> Result<Begun, GoldError> {
    let row = read_run_row(root, run_id).await?;
    if row.code_version != code.as_str() {
        return Err(GoldError::CodeChanged {
            run_id: run_id.clone(),
            begun: row.code_version,
            current: code.as_str().to_string(),
        });
    }
    let changed = Fingerprints::read(root)
        .await
        .map_err(RecordError::from)?
        .changed_since(&Fingerprints::from(row.fingerprints.clone()));
    if !changed.is_empty() {
        return Err(GoldError::DataChanged {
            run_id: run_id.clone(),
            datasets: changed,
        });
    }
    let run = Run::try_from(&row)?;
    let mut sessions = read_run_rows::<EvaluationSessionRowV1>(root, run_id).await?;
    sessions.sort_by(|one, other| one.session_id.cmp(&other.session_id));
    Ok(Begun {
        run,
        date: row.started_at.date_naive(),
        sessions,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use medallion::write_rows;
    use medallion_model::{EvaluationRunRowV2, TrainSessionCrossingRowV1};

    use super::*;
    use crate::fixtures::{instant, oracle, run_of, store};

    fn code(hash: &str) -> CodeVersion {
        CodeVersion::new(hash).expect("a clean code version")
    }

    async fn stored(dir: &Path) -> Root {
        store(dir, 3).await
    }

    async fn written<T: medallion::Row<Layer = medallion::layers::Gold>>(
        root: &Root,
        run_id: &RunId,
    ) -> Vec<T> {
        read_run_rows::<T>(root, run_id)
            .await
            .expect("the run's rows")
    }

    #[tokio::test]
    async fn a_run_scores_the_oracle_perfectly_and_writes_each_dataset() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path()).await;

        let (run_id, ran) = run(&root, &run_of(oracle()), &code("abc1234"))
            .await
            .expect("a run");

        assert_eq!(
            (
                ran.summary.precision,
                ran.summary.recall,
                ran.summary.passes
            ),
            (Some(1.0), Some(1.0), 3)
        );
        assert_eq!(written::<EvaluationRunRowV2>(&root, &run_id).await.len(), 1);
        assert_eq!(
            written::<EvaluationSessionRowV1>(&root, &run_id)
                .await
                .len(),
            3
        );
        assert!(!ran.transitions.is_empty());
        assert_eq!(
            written::<EvaluationTransitionRowV1>(&root, &run_id).await,
            ran.transitions
        );
        assert_eq!(ran.outcomes.len(), 3);
        assert_eq!(
            written::<EvaluationOutcomeRowV1>(&root, &run_id).await,
            ran.outcomes
        );
        assert_eq!(
            written::<EvaluationSummaryRowV1>(&root, &run_id).await,
            [ran.summary]
        );
    }

    #[tokio::test]
    async fn the_steps_run_one_by_one_write_what_a_run_writes() {
        let whole = tempfile::tempdir().expect("a temporary directory");
        let stepwise = tempfile::tempdir().expect("a temporary directory");
        let whole_root = stored(whole.path()).await;
        let stepwise_root = stored(stepwise.path()).await;
        let settings = run_of(oracle());
        let (whole_id, _) = run(&whole_root, &settings, &code("abc1234"))
            .await
            .expect("a run");

        let stepwise_id = pick(&stepwise_root, &settings, &code("abc1234"))
            .await
            .expect("a pick");
        replay(&stepwise_root, &stepwise_id, &code("abc1234"))
            .await
            .expect("a replay");
        score(&stepwise_root, &stepwise_id, &code("abc1234"))
            .await
            .expect("scores");
        summarise(&stepwise_root, &stepwise_id, &code("abc1234"))
            .await
            .expect("a summary");

        assert_eq!(
            written::<EvaluationTransitionRowV1>(&stepwise_root, &stepwise_id).await,
            written::<EvaluationTransitionRowV1>(&whole_root, &whole_id).await
        );
        assert_eq!(
            written::<EvaluationOutcomeRowV1>(&stepwise_root, &stepwise_id).await,
            written::<EvaluationOutcomeRowV1>(&whole_root, &whole_id).await
        );
        assert_eq!(
            written::<EvaluationSummaryRowV1>(&stepwise_root, &stepwise_id).await,
            written::<EvaluationSummaryRowV1>(&whole_root, &whole_id).await
        );
    }

    #[tokio::test]
    async fn a_run_with_the_same_settings_and_inputs_is_refused() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path()).await;
        run(&root, &run_of(oracle()), &code("abc1234"))
            .await
            .expect("a run");

        let refused = run(&root, &run_of(oracle()), &code("abc1234")).await;

        assert!(
            matches!(refused, Err(GoldError::Write(RunError::Written { .. }))),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn a_step_is_refused_once_silver_has_changed_since_the_pick() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path()).await;
        let run_id = pick(&root, &run_of(oracle()), &code("abc1234"))
            .await
            .expect("a pick");
        write_rows(
            &root,
            &[TrainSessionCrossingRowV1 {
                crossed_at: instant(86_400),
                ..medallion::Row::example()
            }],
        )
        .await
        .expect("another pass");

        let refused = replay(&root, &run_id, &code("abc1234")).await;

        assert!(
            matches!(
                &refused,
                Err(GoldError::DataChanged { datasets, .. })
                    if *datasets == ["train_session_crossing_v1"]
            ),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn a_step_is_refused_from_a_build_of_other_code() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path()).await;
        let run_id = pick(&root, &run_of(oracle()), &code("abc1234"))
            .await
            .expect("a pick");

        let refused = replay(&root, &run_id, &code("def5678")).await;

        assert!(
            matches!(&refused, Err(GoldError::CodeChanged { begun, current, .. })
                if begun == "abc1234" && current == "def5678"),
            "{refused:?}"
        );
    }
}
