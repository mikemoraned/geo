use std::collections::BTreeMap;

use chrono::NaiveDate;
use clap::Parser;
use evaluation::runs::{RunsError, read_run_rows};
use medallion::{MedallionArgs, PathError, Query, QueryError, Root, RunError, RunId, write_run};
use medallion_model::{EVALUATION_RUN_V3, EvaluationRunRowV3, EvaluationRunRowV4};
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(about = "Copy each run of gold evaluation_run_v3 into evaluation_run_v4, under its id")]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[derive(Debug, thiserror::Error)]
enum Refused {
    #[error("evaluation_run_v4 already holds run {0}")]
    AlreadyMigrated(RunId),
    #[error("run {run_id} holds {rows} rows in evaluation_run_v3, not one")]
    NotOneRow { run_id: RunId, rows: usize },
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

async fn partitions(root: &Root) -> Result<Vec<Partition>, QueryError> {
    let query = Query::new(root.clone());
    query.register_gold(EVALUATION_RUN_V3).await?;
    query
        .rows(&format!(
            "SELECT DISTINCT CAST(run_date AS VARCHAR) AS run_date, run_id FROM {} ORDER BY run_id",
            EVALUATION_RUN_V3.name
        ))
        .await
}

fn migrated(row: EvaluationRunRowV3) -> EvaluationRunRowV4 {
    EvaluationRunRowV4 {
        started_at: row.started_at,
        evaluation_version: row.evaluation_version,
        code_version: row.code_version,
        country: row.country,
        realtime_at_least: row.realtime_at_least,
        sessions: row.sessions,
        pick_seed: row.pick_seed,
        predictor: row.predictor,
        predictor_settings: row.predictor_settings,
        online_detector_settings: BTreeMap::new(),
        float_type: row.float_type,
        horizon_ms: row.horizon_ms,
        last_moment_ms: row.last_moment_ms,
        ramp_ms: row.ramp_ms,
        last_moment_score: row.last_moment_score,
        floor: row.floor,
        warm_up_ms: row.warm_up_ms,
        resamples: row.resamples,
        bootstrap_seed: row.bootstrap_seed,
        fingerprints: row.fingerprints,
    }
}

async fn migrate(root: &Root, partition: Partition) -> Result<(), Refused> {
    let run_id = RunId::new(partition.run_id)?;
    let date = NaiveDate::parse_from_str(&partition.run_date, "%Y-%m-%d")
        .map_err(|_| Refused::Date(partition.run_date.clone()))?;
    if !read_run_rows::<EvaluationRunRowV4>(root, &run_id)
        .await?
        .is_empty()
    {
        return Err(Refused::AlreadyMigrated(run_id));
    }
    let rows = read_run_rows::<EvaluationRunRowV3>(root, &run_id).await?;
    let [row] = <[EvaluationRunRowV3; 1]>::try_from(rows).map_err(|rows| Refused::NotOneRow {
        run_id: run_id.clone(),
        rows: rows.len(),
    })?;
    write_run(root, date, &run_id, &[migrated(row)]).await?;
    tracing::info!(%run_id, %date, "migrated");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "evaluation_migrate_run=info".into()),
        )
        .init();
    let root = Args::parse().medallion.root()?;
    for partition in partitions(&root).await? {
        migrate(&root, partition).await?;
    }
    Ok(())
}
