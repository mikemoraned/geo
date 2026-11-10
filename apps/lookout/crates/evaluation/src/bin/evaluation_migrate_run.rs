use chrono::{NaiveDate, TimeDelta};
use clap::Parser;
use domain::{Radius, Span};
use evaluation::runs::{RunsError, read_run_rows};
use evaluation::steps::Predictor;
use evaluation::{CrowFliesSettings, OracleSettings};
use medallion::{MedallionArgs, PathError, Query, QueryError, Root, RunError, RunId, write_run};
use medallion_model::{EVALUATION_RUN_V2, EvaluationRunRowV2, EvaluationRunRowV3, PredictorName};
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(about = "Copy each run of gold evaluation_run_v2 into evaluation_run_v3, under its id")]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[derive(Debug, thiserror::Error)]
enum Refused {
    #[error("evaluation_run_v3 already holds run {0}")]
    AlreadyMigrated(RunId),
    #[error("run {run_id} holds {rows} rows in evaluation_run_v2, not one")]
    NotOneRow { run_id: RunId, rows: usize },
    #[error("run {run_id} has no `{column}`, which its predictor needs")]
    Missing { run_id: RunId, column: &'static str },
    #[error("run {run_id}'s `{column}` is no setting: {reason}")]
    Unreadable {
        run_id: RunId,
        column: &'static str,
        reason: String,
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

async fn partitions(root: &Root) -> Result<Vec<Partition>, QueryError> {
    let query = Query::new(root.clone());
    query.register_gold(EVALUATION_RUN_V2).await?;
    query
        .rows(&format!(
            "SELECT DISTINCT CAST(run_date AS VARCHAR) AS run_date, run_id FROM {} ORDER BY run_id",
            EVALUATION_RUN_V2.name
        ))
        .await
}

fn predictor(run_id: &RunId, row: &EvaluationRunRowV2) -> Result<Predictor, Refused> {
    let missing = |column| Refused::Missing {
        run_id: run_id.clone(),
        column,
    };
    let unreadable = |column, reason: String| Refused::Unreadable {
        run_id: run_id.clone(),
        column,
        reason,
    };
    let horizon = Span::try_from(TimeDelta::milliseconds(
        row.predictor_horizon_ms
            .ok_or_else(|| missing("predictor_horizon_ms"))?,
    ))
    .map_err(|err| unreadable("predictor_horizon_ms", err.to_string()))?;
    Ok(match row.predictor {
        PredictorName::Oracle => Predictor::Oracle(OracleSettings { horizon }),
        PredictorName::CrowFlies => Predictor::CrowFlies(CrowFliesSettings {
            radius: Radius::new(
                row.predictor_radius_m
                    .ok_or_else(|| missing("predictor_radius_m"))?,
            )
            .map_err(|err| unreadable("predictor_radius_m", err.to_string()))?,
            horizon,
        }),
    })
}

fn migrated(run_id: &RunId, row: EvaluationRunRowV2) -> Result<EvaluationRunRowV3, Refused> {
    let predictor = predictor(run_id, &row)?;
    Ok(EvaluationRunRowV3 {
        started_at: row.started_at,
        evaluation_version: row.evaluation_version,
        code_version: row.code_version,
        country: row.country,
        realtime_at_least: row.realtime_at_least,
        sessions: row.sessions,
        pick_seed: row.pick_seed,
        predictor: predictor.name(),
        predictor_settings: predictor.settings(),
        pass_detector: row.pass_detector,
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
    })
}

async fn migrate(root: &Root, partition: Partition) -> Result<(), Refused> {
    let run_id = RunId::new(partition.run_id)?;
    let date = NaiveDate::parse_from_str(&partition.run_date, "%Y-%m-%d")
        .map_err(|_| Refused::Date(partition.run_date.clone()))?;
    if !read_run_rows::<EvaluationRunRowV3>(root, &run_id)
        .await?
        .is_empty()
    {
        return Err(Refused::AlreadyMigrated(run_id));
    }
    let rows = read_run_rows::<EvaluationRunRowV2>(root, &run_id).await?;
    let [row] = <[EvaluationRunRowV2; 1]>::try_from(rows).map_err(|rows| Refused::NotOneRow {
        run_id: run_id.clone(),
        rows: rows.len(),
    })?;
    let row = migrated(&run_id, row)?;
    write_run(root, date, &run_id, &[row]).await?;
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
