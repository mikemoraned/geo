use chrono::{NaiveDate, TimeDelta};
use clap::Parser;
use evaluation::runs::{RunsError, read_run_rows};
use medallion::{
    DatasetSpec, MedallionArgs, PathError, Query, QueryError, Root, Row, RunError, RunId, layers,
    write_run,
};
use medallion_model::{
    EvaluationOutcomeRowV1, EvaluationOutcomeRowV2, EvaluationSkewRowV1, EvaluationSkewRowV2,
    EvaluationSummaryRowV1, EvaluationSummaryRowV2,
};
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    about = "Copy each run of gold evaluation_outcome_v1, evaluation_summary_v1, and \
             evaluation_skew_v1 into its v2, under its id, with each span of time as a duration"
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

fn duration(milliseconds: Option<i64>) -> Option<TimeDelta> {
    milliseconds.map(TimeDelta::milliseconds)
}

fn outcome(row: EvaluationOutcomeRowV1) -> EvaluationOutcomeRowV2 {
    EvaluationOutcomeRowV2 {
        session_id: row.session_id,
        crossing_compact_id: row.crossing_compact_id,
        outcome: row.outcome,
        notified_at: row.notified_at,
        crossed_at: row.crossed_at,
        lead: duration(row.lead_ms),
        class: row.class,
        score: row.score,
    }
}

fn summary(row: EvaluationSummaryRowV1) -> EvaluationSummaryRowV2 {
    EvaluationSummaryRowV2 {
        sessions: row.sessions,
        scored_duration: TimeDelta::milliseconds((row.hours * 3_600_000.0).round() as i64),
        passes: row.passes,
        notifications: row.notifications,
        false_alarms: row.false_alarms,
        repeats: row.repeats,
        misses: row.misses,
        precision: row.precision,
        precision_low: row.precision_low,
        precision_high: row.precision_high,
        recall: row.recall,
        recall_low: row.recall_low,
        recall_high: row.recall_high,
        false_alarms_per_hour: row.false_alarms_per_hour,
        false_alarms_per_hour_low: row.false_alarms_per_hour_low,
        false_alarms_per_hour_high: row.false_alarms_per_hour_high,
        lead_p10: duration(row.lead_p10_ms),
        lead_median: duration(row.lead_median_ms),
        lead_p90: duration(row.lead_p90_ms),
    }
}

fn skew(row: EvaluationSkewRowV1) -> EvaluationSkewRowV2 {
    EvaluationSkewRowV2 {
        sessions: row.sessions,
        passes: row.passes,
        visits: row.visits,
        seen_passes: row.seen_passes,
        unmatched_visits: row.unmatched_visits,
        seen: row.seen,
        seen_low: row.seen_low,
        seen_high: row.seen_high,
        unmatched: row.unmatched,
        unmatched_low: row.unmatched_low,
        unmatched_high: row.unmatched_high,
        offset_p10: duration(row.offset_p10_ms),
        offset_median: duration(row.offset_median_ms),
        offset_p90: duration(row.offset_p90_ms),
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
                .unwrap_or_else(|_| "evaluation_migrate_durations=info".into()),
        )
        .init();
    let root = Args::parse().medallion.root()?;
    migrate(&root, outcome).await?;
    migrate(&root, summary).await?;
    migrate(&root, skew).await?;
    Ok(())
}
