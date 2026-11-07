use std::path::Path;

use evaluate::{Commit, EvaluateError};
use evaluation::fixtures::{oracle, run_of, store};
use evaluation::runs::read_run_rows;
use medallion::{Root, RunError, RunId, write_rows};
use medallion_model::{
    EvaluationOutcomeRowV1, EvaluationRunRowV1, EvaluationSessionRowV1, EvaluationSummaryRowV1,
    EvaluationTransitionRowV1, TrainSessionCrossingRowV1,
};

fn commit(hash: &str) -> Commit {
    Commit::new(hash).expect("a clean commit")
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

    let (run_id, summary) = evaluate::run(&root, &run_of(oracle()), &commit("abc1234"))
        .await
        .expect("a run");

    assert_eq!(
        (summary.precision, summary.recall, summary.passes),
        (Some(1.0), Some(1.0), 3)
    );
    assert_eq!(written::<EvaluationRunRowV1>(&root, &run_id).await.len(), 1);
    assert_eq!(
        written::<EvaluationSessionRowV1>(&root, &run_id)
            .await
            .len(),
        3
    );
    assert!(
        !written::<EvaluationTransitionRowV1>(&root, &run_id)
            .await
            .is_empty()
    );
    assert_eq!(
        written::<EvaluationOutcomeRowV1>(&root, &run_id)
            .await
            .len(),
        3
    );
    assert_eq!(
        written::<EvaluationSummaryRowV1>(&root, &run_id).await,
        [summary]
    );
}

#[tokio::test]
async fn the_steps_run_one_by_one_write_what_a_run_writes() {
    let whole = tempfile::tempdir().expect("a temporary directory");
    let stepwise = tempfile::tempdir().expect("a temporary directory");
    let whole_root = stored(whole.path()).await;
    let stepwise_root = stored(stepwise.path()).await;
    let settings = run_of(oracle());
    let (whole_id, _) = evaluate::run(&whole_root, &settings, &commit("abc1234"))
        .await
        .expect("a run");

    let stepwise_id = evaluate::pick(&stepwise_root, &settings, &commit("abc1234"))
        .await
        .expect("a pick");
    evaluate::replay(&stepwise_root, &stepwise_id, &commit("abc1234"))
        .await
        .expect("a replay");
    evaluate::score(&stepwise_root, &stepwise_id, &commit("abc1234"))
        .await
        .expect("scores");
    evaluate::summarise(&stepwise_root, &stepwise_id, &commit("abc1234"))
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
    evaluate::run(&root, &run_of(oracle()), &commit("abc1234"))
        .await
        .expect("a run");

    let refused = evaluate::run(&root, &run_of(oracle()), &commit("abc1234")).await;

    assert!(
        matches!(refused, Err(EvaluateError::Write(RunError::Written { .. }))),
        "{refused:?}"
    );
}

#[tokio::test]
async fn a_step_is_refused_once_silver_has_changed_since_the_pick() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let root = stored(tmp.path()).await;
    let run_id = evaluate::pick(&root, &run_of(oracle()), &commit("abc1234"))
        .await
        .expect("a pick");
    write_rows(
        &root,
        &[TrainSessionCrossingRowV1 {
            crossed_at: evaluation::fixtures::instant(86_400),
            ..medallion::Row::example()
        }],
    )
    .await
    .expect("another pass");

    let refused = evaluate::replay(&root, &run_id, &commit("abc1234")).await;

    assert!(
        matches!(
            &refused,
            Err(EvaluateError::DataChanged { datasets, .. })
                if *datasets == ["train_session_crossing_v1"]
        ),
        "{refused:?}"
    );
}

#[tokio::test]
async fn a_step_is_refused_from_a_build_of_another_commit() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let root = stored(tmp.path()).await;
    let run_id = evaluate::pick(&root, &run_of(oracle()), &commit("abc1234"))
        .await
        .expect("a pick");

    let refused = evaluate::replay(&root, &run_id, &commit("def5678")).await;

    assert!(
        matches!(&refused, Err(EvaluateError::CommitChanged { begun, current, .. })
            if begun == "abc1234" && current == "def5678"),
        "{refused:?}"
    );
}

#[test]
fn a_build_with_uncommitted_changes_or_no_commit_is_refused() {
    assert!(matches!(
        Commit::new("abc1234-dirty"),
        Err(EvaluateError::Uncommitted { .. })
    ));
    assert!(matches!(
        Commit::new("unknown"),
        Err(EvaluateError::NoCommit)
    ));
    assert!(matches!(Commit::new(""), Err(EvaluateError::NoCommit)));
}
