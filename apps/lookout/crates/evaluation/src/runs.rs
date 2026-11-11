use datafusion::prelude::{col, lit};
use medallion::{Query, QueryError, RUN_ID, Root, Row, RunId, layers};
use medallion_model::EvaluationRunRowV4;

#[derive(Debug, thiserror::Error)]
pub enum RunsError {
    #[error("gold holds no run {run_id}")]
    NoSuchRun { run_id: RunId },
    #[error("reading gold: {0}")]
    Query(#[from] QueryError),
}

pub async fn read_run_row(root: &Root, run_id: &RunId) -> Result<EvaluationRunRowV4, RunsError> {
    read_run_rows::<EvaluationRunRowV4>(root, run_id)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| RunsError::NoSuchRun {
            run_id: run_id.clone(),
        })
}

pub async fn read_run_rows<T: Row<Layer = layers::Gold>>(
    root: &Root,
    run_id: &RunId,
) -> Result<Vec<T>, RunsError> {
    let read = match Query::new(root.clone()).read::<T>().await {
        Err(QueryError::NoSuchDataset { .. }) => return Ok(Vec::new()),
        read => read?,
    };
    Ok(read
        .filter(col(RUN_ID).eq(lit(run_id.to_string())))?
        .rows()
        .await?)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::NaiveDate;
    use domain::StateKind;
    use domain::{CrossingCompactId, SessionId};
    use medallion::write_run;
    use medallion_model::EvaluationTransitionRowV1;

    use super::*;
    use crate::fixtures::instant;

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 6).expect("a date")
    }

    fn run(id: &str) -> RunId {
        RunId::new(id).expect("a run id")
    }

    fn session(name: &str) -> SessionId {
        SessionId::new(format!("session-{name}")).expect("a session id")
    }

    fn run_row(code: &str) -> EvaluationRunRowV4 {
        EvaluationRunRowV4 {
            code_version: code.to_string(),
            ..EvaluationRunRowV4::example()
        }
    }

    fn transition(name: &str) -> EvaluationTransitionRowV1 {
        EvaluationTransitionRowV1 {
            session_id: session(name),
            crossing_compact_id: CrossingCompactId::new(1),
            at: instant(0),
            from: StateKind::OutOfReach,
            to: StateKind::AlertedPass,
        }
    }

    async fn two_runs(dir: &Path) -> Root {
        let root = Root::new(dir);
        for (id, code, name) in [("abc", "code-a", "a"), ("def", "code-d", "d")] {
            write_run(&root, day(), &run(id), &[run_row(code)])
                .await
                .expect("a written run");
            write_run(&root, day(), &run(id), &[transition(name)])
                .await
                .expect("written transitions");
        }
        root
    }

    #[tokio::test]
    async fn a_runs_row_is_read_back_by_its_id() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = two_runs(tmp.path()).await;

        let row = read_run_row(&root, &run("def")).await.expect("the run");

        assert_eq!(row, run_row("code-d"));
    }

    #[tokio::test]
    async fn a_run_gold_does_not_hold_is_refused() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = two_runs(tmp.path()).await;
        let empty = tempfile::tempdir().expect("a temporary directory");

        for (root, id) in [(&root, "xyz"), (&Root::new(empty.path()), "abc")] {
            let refused = read_run_row(root, &run(id)).await;

            assert!(
                matches!(&refused, Err(RunsError::NoSuchRun { run_id }) if *run_id == run(id)),
                "{refused:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_runs_rows_of_a_dataset_are_read_without_another_runs() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = two_runs(tmp.path()).await;

        let transitions = read_run_rows::<EvaluationTransitionRowV1>(&root, &run("abc"))
            .await
            .expect("the run's transitions");

        assert_eq!(transitions, [transition("a")]);
    }
}
