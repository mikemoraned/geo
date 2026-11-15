use std::collections::BTreeMap;

use chrono::{NaiveDate, TimeDelta};
use clap::Parser;
use evaluation::runs::{RunsError, read_run_rows};
use medallion::{
    DatasetSpec, MedallionArgs, PathError, Query, QueryError, Root, Row, RunError, RunId, layers,
    write_run,
};
use medallion_model::{
    EvaluationRunRowV4, EvaluationRunRowV5, EvaluationSessionRowV1, EvaluationSessionRowV2,
};
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    about = "Copy each run of gold evaluation_run_v4 and evaluation_session_v1 into \
             evaluation_run_v5 and evaluation_session_v2, under its id, with no split and each \
             span of time as a duration"
)]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[derive(Debug, thiserror::Error)]
enum Refused {
    #[error("{dataset} already holds run {run_id}")]
    AlreadyMigrated {
        dataset: &'static str,
        run_id: RunId,
    },
    #[error("run date `{0}` is no date")]
    Date(String),
    #[error(transparent)]
    Query(#[from] QueryError),
    #[error(transparent)]
    Runs(#[from] RunsError),
    #[error(transparent)]
    Path(#[from] PathError),
    #[error(transparent)]
    Run(#[from] RunError),
}

#[derive(Deserialize)]
struct Partition {
    run_date: String,
    run_id: String,
}

async fn partitions(
    root: &Root,
    dataset: DatasetSpec<layers::Gold>,
) -> Result<Vec<Partition>, QueryError> {
    let query = Query::new(root.clone());
    query.register_gold(dataset).await?;
    query
        .rows(&format!(
            "SELECT DISTINCT CAST(run_date AS VARCHAR) AS run_date, run_id FROM {} ORDER BY run_id",
            dataset.name
        ))
        .await
}

fn run(row: EvaluationRunRowV4) -> EvaluationRunRowV5 {
    EvaluationRunRowV5 {
        started_at: row.started_at,
        evaluation_version: row.evaluation_version,
        code_version: row.code_version,
        country: row.country,
        realtime_at_least: row.realtime_at_least,
        sessions: row.sessions,
        pick_seed: row.pick_seed,
        predictor: row.predictor,
        predictor_settings: row.predictor_settings,
        online_detector_settings: row.online_detector_settings,
        split_settings: BTreeMap::new(),
        float_type: row.float_type,
        horizon: TimeDelta::milliseconds(row.horizon_ms),
        last_moment: TimeDelta::milliseconds(row.last_moment_ms),
        ramp: TimeDelta::milliseconds(row.ramp_ms),
        last_moment_score: row.last_moment_score,
        floor: row.floor,
        warm_up: TimeDelta::milliseconds(row.warm_up_ms),
        resamples: row.resamples,
        bootstrap_seed: row.bootstrap_seed,
        fingerprints: row.fingerprints,
    }
}

fn session(row: EvaluationSessionRowV1) -> EvaluationSessionRowV2 {
    let duration = row.ended_at - row.started_at;
    EvaluationSessionRowV2 {
        session_id: row.session_id,
        started_at: row.started_at,
        ended_at: row.ended_at,
        duration,
        scored_duration: duration,
    }
}

async fn migrate<Old, New>(root: &Root, migrated: fn(Old) -> New) -> Result<(), Refused>
where
    Old: Row<Layer = layers::Gold>,
    New: Row<Layer = layers::Gold>,
{
    for partition in partitions(root, Old::DATASET).await? {
        let run_id = RunId::new(partition.run_id)?;
        let date = NaiveDate::parse_from_str(&partition.run_date, "%Y-%m-%d")
            .map_err(|_| Refused::Date(partition.run_date.clone()))?;
        if !read_run_rows::<New>(root, &run_id).await?.is_empty() {
            return Err(Refused::AlreadyMigrated {
                dataset: New::DATASET.name,
                run_id,
            });
        }
        let rows: Vec<New> = read_run_rows::<Old>(root, &run_id)
            .await?
            .into_iter()
            .map(migrated)
            .collect();
        write_run(root, date, &run_id, &rows).await?;
        tracing::info!(dataset = New::DATASET.name, %run_id, %date, rows = rows.len(), "migrated");
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "evaluation_migrate_run_v5=info".into()),
        )
        .init();
    let root = Args::parse().medallion.root()?;
    migrate(&root, run).await?;
    migrate(&root, session).await?;
    Ok(())
}
