use chrono::NaiveDate;
use medallion::{Root, RunError, RunId, write_run};
use medallion_model::{
    EvaluationEpisodeRowV1, EvaluationOutcomeRowV3, EvaluationSessionRowV3, EvaluationSkewRowV2,
    EvaluationSummaryRowV3, TrainSessionRowV3,
};

use crate::PickError;
use crate::episodes::{EpisodeError, episode_rows, transition_rows_of};
use crate::fingerprints::Fingerprints;
use crate::outcomes::{CompactError, compact_outcome_rows, outcome_rows_of};
use code_version::CodeVersion;

use crate::record::{RecordError, RecordResults, record};
use crate::runs::{RunsError, read_run_row, read_run_rows};
use crate::steps::{
    self, EvaluationResults, EvaluationSettings, ReplayResults, ScoreResults, StepError,
};

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
    Episode(#[from] EpisodeError),
    #[error(transparent)]
    Compact(#[from] CompactError),
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

pub async fn pick(
    root: &Root,
    run: &EvaluationSettings,
    code: &CodeVersion,
) -> Result<RunId, GoldError> {
    let picked = crate::pick(root, &run.pick).await?;
    Ok(begin(root, run, code, &picked).await?.run_id)
}

pub async fn replay(
    root: &Root,
    run_id: &RunId,
    code: &CodeVersion,
) -> Result<Option<EvaluationSkewRowV2>, GoldError> {
    let begun = resume(root, run_id, code).await?;
    let replayed = steps::replay(
        root,
        begun.run.pick.country,
        &begun.sessions,
        begun.run.predictor,
        begun.run.float_type,
        begun.run.skew_settings(),
        begun.run.split.as_ref(),
    )
    .await?;
    write_replayed(root, begun.date, run_id, &begun.sessions, &replayed).await?;
    Ok(replayed.skew)
}

async fn write_replayed(
    root: &Root,
    date: NaiveDate,
    run_id: &RunId,
    sessions: &[EvaluationSessionRowV3],
    replayed: &ReplayResults,
) -> Result<(), GoldError> {
    let episodes = episode_rows(sessions, &replayed.transitions)?;
    tracing::info!(%run_id, transitions = replayed.transitions.len(), episodes = episodes.len(), "replayed");
    write_run(root, date, run_id, &episodes).await?;
    if let Some(skew) = &replayed.skew {
        tracing::info!(%run_id, "measured the skew");
        write_run(root, date, run_id, std::slice::from_ref(skew)).await?;
    }
    Ok(())
}

pub async fn score(root: &Root, run_id: &RunId, code: &CodeVersion) -> Result<(), GoldError> {
    let begun = resume(root, run_id, code).await?;
    let episodes = read_run_rows::<EvaluationEpisodeRowV1>(root, run_id).await?;
    let transitions = transition_rows_of(&begun.sessions, &episodes)?;
    let scored = steps::score(
        root,
        begun.run.pick.country,
        &begun.sessions,
        &transitions,
        &begun.run.scoring,
        begun.run.split.as_ref(),
    )
    .await?;
    write_scored(root, begun.date, run_id, &begun.sessions, &scored).await
}

async fn write_scored(
    root: &Root,
    date: NaiveDate,
    run_id: &RunId,
    sessions: &[EvaluationSessionRowV3],
    scored: &ScoreResults,
) -> Result<(), GoldError> {
    tracing::info!(%run_id, outcomes = scored.outcomes.len(), "scored");
    write_run(
        root,
        date,
        run_id,
        &compact_outcome_rows(sessions, &scored.outcomes)?,
    )
    .await?;
    if let Some(split) = &scored.split {
        tracing::info!(%run_id, "checked the split");
        write_run(root, date, run_id, std::slice::from_ref(split)).await?;
    }
    Ok(())
}

pub async fn summarise(
    root: &Root,
    run_id: &RunId,
    code: &CodeVersion,
) -> Result<EvaluationSummaryRowV3, GoldError> {
    let begun = resume(root, run_id, code).await?;
    let compact = read_run_rows::<EvaluationOutcomeRowV3>(root, run_id).await?;
    let outcomes = outcome_rows_of(&begun.sessions, &compact)?;
    let summary = steps::summarise(&begun.sessions, &outcomes, &begun.run.bootstrap)?;
    tracing::info!(%run_id, "summarised");
    write_run(root, begun.date, run_id, std::slice::from_ref(&summary)).await?;
    Ok(summary)
}

pub async fn run(
    root: &Root,
    run: &EvaluationSettings,
    code: &CodeVersion,
) -> Result<(RunId, EvaluationResults), GoldError> {
    let country = run.pick.country;
    let picked = crate::pick(root, &run.pick).await?;
    let RecordResults {
        run_id,
        date,
        sessions,
        ..
    } = begin(root, run, code, &picked).await?;

    let crossings_and_passes =
        steps::read_crossings_and_passes(root, country, &sessions, run.split.as_ref()).await?;
    let replayed = steps::replay_against(
        root,
        country,
        &sessions,
        &crossings_and_passes,
        run.predictor,
        run.float_type,
        run.skew_settings(),
    )
    .await?;
    write_replayed(root, date, &run_id, &sessions, &replayed).await?;
    let ReplayResults { transitions, skew } = replayed;

    let scored =
        steps::score_against(&sessions, &crossings_and_passes, &transitions, &run.scoring)?;
    write_scored(root, date, &run_id, &sessions, &scored).await?;
    let ScoreResults { outcomes, split } = scored;

    let summary = steps::summarise(&sessions, &outcomes, &run.bootstrap)?;
    tracing::info!(%run_id, "summarised");
    write_run(root, date, &run_id, std::slice::from_ref(&summary)).await?;

    Ok((
        run_id,
        EvaluationResults {
            picked,
            transitions,
            skew,
            outcomes,
            split,
            summary,
        },
    ))
}

async fn begin(
    root: &Root,
    run: &EvaluationSettings,
    code: &CodeVersion,
    picked: &[TrainSessionRowV3],
) -> Result<RecordResults, GoldError> {
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
    run: EvaluationSettings,
    date: NaiveDate,
    sessions: Vec<EvaluationSessionRowV3>,
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
    let run = EvaluationSettings::try_from(&row)?;
    let mut sessions = read_run_rows::<EvaluationSessionRowV3>(root, run_id).await?;
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
    use medallion_model::{EvaluationRunRowV6, EvaluationSplitRowV1, TrainSessionCrossingRowV1};

    use super::*;
    use crate::fixtures::{around_dresden_hbf, instant, oracle, run_of, store, write_country};
    use crate::split::{ScoredSplit, Split, Splitter};

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

        let (run_id, results) = run(&root, &run_of(oracle()), &code("abc1234"))
            .await
            .expect("a run");

        assert_eq!(
            (
                results.summary.precision,
                results.summary.recall,
                results.summary.passes
            ),
            (Some(1.0), Some(1.0), 3)
        );
        assert_eq!(written::<EvaluationRunRowV6>(&root, &run_id).await.len(), 1);
        assert_eq!(
            written::<EvaluationSessionRowV3>(&root, &run_id)
                .await
                .len(),
            3
        );
        assert!(!results.transitions.is_empty());
        let sessions = written::<EvaluationSessionRowV3>(&root, &run_id).await;
        assert_eq!(
            transition_rows_of(
                &sessions,
                &written::<EvaluationEpisodeRowV1>(&root, &run_id).await
            )
            .expect("transitions"),
            results.transitions
        );
        assert_eq!(results.outcomes.len(), 3);
        assert_eq!(
            outcome_rows_of(
                &sessions,
                &written::<EvaluationOutcomeRowV3>(&root, &run_id).await
            )
            .expect("outcomes"),
            outcome_rows_of(
                &sessions,
                &compact_outcome_rows(&sessions, &results.outcomes).expect("compact outcomes")
            )
            .expect("outcomes")
        );
        assert_eq!(
            written::<EvaluationSummaryRowV3>(&root, &run_id).await,
            [results.summary]
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
            written::<EvaluationEpisodeRowV1>(&stepwise_root, &stepwise_id).await,
            written::<EvaluationEpisodeRowV1>(&whole_root, &whole_id).await
        );
        assert_eq!(
            written::<EvaluationOutcomeRowV3>(&stepwise_root, &stepwise_id).await,
            written::<EvaluationOutcomeRowV3>(&whole_root, &whole_id).await
        );
        assert_eq!(
            written::<EvaluationSummaryRowV3>(&stepwise_root, &stepwise_id).await,
            written::<EvaluationSummaryRowV3>(&whole_root, &whole_id).await
        );
    }

    #[tokio::test]
    async fn a_run_with_a_split_writes_its_checks_counts_whether_run_whole_or_step_by_step() {
        let whole = tempfile::tempdir().expect("a temporary directory");
        let stepwise = tempfile::tempdir().expect("a temporary directory");
        let whole_root = stored(whole.path()).await;
        let stepwise_root = stored(stepwise.path()).await;
        write_country(&whole_root, around_dresden_hbf(1.0)).await;
        write_country(&stepwise_root, around_dresden_hbf(1.0)).await;
        let settings = EvaluationSettings {
            split: Some(ScoredSplit {
                splitter: Splitter {
                    resolution: "8".parse().expect("a resolution"),
                    salt: "tune-test-1".parse().expect("a salt"),
                    tune_buckets: "70".parse().expect("tune buckets"),
                },
                split: Split::Test,
            }),
            ..run_of(oracle())
        };
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

        let expected = [EvaluationSplitRowV1 {
            cells: 2,
            cells_in_split: 1,
            crossings: 2,
            crossings_in_split: 1,
            passes_in_split: 3,
            passes_outside: 0,
            outcomes_in_split: 3,
            outcomes_outside: 0,
            cells_outside_cover: 0,
        }];
        assert_eq!(
            written::<EvaluationSplitRowV1>(&whole_root, &whole_id).await,
            expected
        );
        assert_eq!(
            written::<EvaluationSplitRowV1>(&stepwise_root, &stepwise_id).await,
            expected
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
