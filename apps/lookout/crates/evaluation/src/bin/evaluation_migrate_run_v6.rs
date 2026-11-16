use chrono::NaiveDate;
use clap::Parser;
use evaluation::runs::{RunsError, read_run_rows};
use medallion::{
    DatasetSpec, MedallionArgs, PathError, Query, QueryError, Root, Row, RunError, RunId, layers,
    write_run,
};
use medallion_model::{EvaluationRunRowV5, EvaluationRunRowV6, PredictorName, PredictorNameV5};
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(about = "Copy each run of gold evaluation_run_v5 into evaluation_run_v6, under its id")]
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

fn predictor(name: PredictorNameV5) -> PredictorName {
    match name {
        PredictorNameV5::Oracle => PredictorName::Oracle,
        PredictorNameV5::CrowFlies => PredictorName::CrowFlies,
    }
}

fn run(row: EvaluationRunRowV5) -> EvaluationRunRowV6 {
    EvaluationRunRowV6 {
        started_at: row.started_at,
        evaluation_version: row.evaluation_version,
        code_version: row.code_version,
        country: row.country,
        realtime_at_least: row.realtime_at_least,
        sessions: row.sessions,
        pick_seed: row.pick_seed,
        predictor: predictor(row.predictor),
        predictor_settings: row.predictor_settings,
        online_detector_settings: row.online_detector_settings,
        split_settings: row.split_settings,
        float_type: row.float_type,
        horizon: row.horizon,
        last_moment: row.last_moment,
        ramp: row.ramp,
        last_moment_score: row.last_moment_score,
        floor: row.floor,
        warm_up: row.warm_up,
        resamples: row.resamples,
        bootstrap_seed: row.bootstrap_seed,
        fingerprints: row.fingerprints,
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
                .unwrap_or_else(|_| "evaluation_migrate_run_v6=info".into()),
        )
        .init();
    let root = Args::parse().medallion.root()?;
    migrate(&root, run).await?;
    Ok(())
}
