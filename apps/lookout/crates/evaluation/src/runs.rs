use datafusion::prelude::{col, lit};
use medallion::{Country, Query, QueryError, RUN_ID, Root, Row, RunId, layers};
use medallion_model::{EvaluationRunRowV1, EvaluationSessionRowV1, TrainSessionRowV3};

#[derive(Debug, thiserror::Error)]
pub enum RunsError {
    #[error("gold holds no run {run_id}")]
    NoSuchRun { run_id: RunId },
    #[error("reading gold: {0}")]
    Query(#[from] QueryError),
}

pub async fn read_run_row(root: &Root, run_id: &RunId) -> Result<EvaluationRunRowV1, RunsError> {
    read_run_rows::<EvaluationRunRowV1>(root, run_id)
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

pub async fn read_sessions(
    root: &Root,
    country: Country,
    sessions: &[EvaluationSessionRowV1],
) -> Result<Vec<TrainSessionRowV3>, RunsError> {
    let picked = col("session_id").in_list(
        sessions
            .iter()
            .map(|session| lit(session.session_id.to_string()))
            .collect(),
        false,
    );
    Ok(Query::new(root.clone())
        .read_of_country::<TrainSessionRowV3>(country)
        .await?
        .filter(picked)?
        .sort(vec![col("session_id").sort(true, false)])?
        .rows()
        .await?)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::{DateTime, NaiveDate, TimeDelta, Utc};
    use domain::StateKind;
    use domain::{CrossingCompactId, SessionId};
    use geo_types::LineString;
    use medallion::{GeographyRow, write_geography_rows, write_run};
    use medallion_model::EvaluationTransitionRowV1;

    use super::*;
    use domain::places::DRESDEN_HBF;

    fn instant(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
            + TimeDelta::seconds(seconds)
    }

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 6).expect("a date")
    }

    fn run(id: &str) -> RunId {
        RunId::new(id).expect("a run id")
    }

    fn session(name: &str) -> SessionId {
        SessionId::new(format!("session-{name}")).expect("a session id")
    }

    fn run_row(code: &str) -> EvaluationRunRowV1 {
        EvaluationRunRowV1 {
            code_version: code.to_string(),
            ..EvaluationRunRowV1::example()
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

    #[tokio::test]
    async fn the_picked_sessions_are_read_again_from_silver_in_order_of_id() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = Root::new(tmp.path());
        let line = LineString::from(vec![
            DRESDEN_HBF.x_y(),
            (DRESDEN_HBF.x(), DRESDEN_HBF.y() + 0.01),
        ]);
        let stored: Vec<GeographyRow<TrainSessionRowV3, LineString<f64>>> = ["c", "a", "b"]
            .into_iter()
            .map(|name| GeographyRow {
                row: TrainSessionRowV3 {
                    session_id: session(name),
                    ..TrainSessionRowV3::example()
                },
                geography: line.clone(),
                country: Country::Germany,
            })
            .collect();
        write_geography_rows(&root, &stored)
            .await
            .expect("written sessions");
        let picked: Vec<EvaluationSessionRowV1> = ["c", "a"]
            .into_iter()
            .map(|name| EvaluationSessionRowV1 {
                session_id: session(name),
                ..EvaluationSessionRowV1::example()
            })
            .collect();

        let sessions = read_sessions(&root, Country::Germany, &picked)
            .await
            .expect("the sessions");

        assert_eq!(
            sessions
                .iter()
                .map(|session| session.session_id.clone())
                .collect::<Vec<_>>(),
            [session("a"), session("c")]
        );
    }
}
