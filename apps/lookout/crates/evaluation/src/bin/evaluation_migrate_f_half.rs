use std::num::NonZeroUsize;

use chrono::NaiveDate;
use clap::Parser;
use evaluation::runs::{RunsError, read_run_row, read_run_rows};
use evaluation::steps::{self, StepError};
use evaluation::{Bootstrap, Seed};
use medallion::{
    DatasetSpec, MedallionArgs, PathError, Query, QueryError, Root, Row, RunError, RunId, layers,
    write_run,
};
use medallion_model::{
    EvaluationOutcomeRowV2, EvaluationSessionRowV2, EvaluationSummaryRowV2, EvaluationSummaryRowV3,
};
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    about = "Summarise each run of gold evaluation_summary_v2 again from its outcomes into \
             evaluation_summary_v3, under its id, refusing any run whose measures change"
)]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[derive(Debug, thiserror::Error)]
enum Refused {
    #[error("evaluation_summary_v3 already holds run {0}")]
    AlreadyMigrated(RunId),
    #[error("run {0} holds {1} rows of evaluation_summary_v2, not one")]
    NotOneSummary(RunId, usize),
    #[error("run {run_id} summarises to {again:?}, against {before:?} in evaluation_summary_v2")]
    Changed {
        run_id: RunId,
        before: Box<EvaluationSummaryRowV2>,
        again: Box<EvaluationSummaryRowV3>,
    },
    #[error("run {0} draws no resamples")]
    NoResamples(RunId),
    #[error("run date `{0}` is no date")]
    Date(String),
    #[error(transparent)]
    Query(#[from] QueryError),
    #[error(transparent)]
    Runs(#[from] RunsError),
    #[error(transparent)]
    Step(#[from] StepError),
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

fn close(before: Option<f64>, again: Option<f64>) -> bool {
    match (before, again) {
        (Some(before), Some(again)) => (before - again).abs() <= 1e-12 * before.abs().max(1.0),
        (before, again) => before == again,
    }
}

fn unchanged(before: &EvaluationSummaryRowV2, again: &EvaluationSummaryRowV3) -> bool {
    let counts = |row: &EvaluationSummaryRowV2| {
        (
            row.sessions,
            row.scored_duration,
            row.passes,
            row.notifications,
            row.false_alarms,
            row.repeats,
            row.misses,
            row.lead_p10,
            row.lead_median,
            row.lead_p90,
        )
    };
    let counted_again = (
        again.sessions,
        again.scored_duration,
        again.passes,
        again.notifications,
        again.false_alarms,
        again.repeats,
        again.misses,
        again.lead_p10,
        again.lead_median,
        again.lead_p90,
    );
    counts(before) == counted_again
        && [
            (before.precision, again.precision),
            (before.precision_low, again.precision_low),
            (before.precision_high, again.precision_high),
            (before.recall, again.recall),
            (before.recall_low, again.recall_low),
            (before.recall_high, again.recall_high),
            (before.false_alarms_per_hour, again.false_alarms_per_hour),
            (
                before.false_alarms_per_hour_low,
                again.false_alarms_per_hour_low,
            ),
            (
                before.false_alarms_per_hour_high,
                again.false_alarms_per_hour_high,
            ),
        ]
        .into_iter()
        .all(|(before, again)| close(before, again))
}

async fn migrate(root: &Root, partition: Partition) -> Result<(), Refused> {
    let run_id = RunId::new(partition.run_id)?;
    let date = NaiveDate::parse_from_str(&partition.run_date, "%Y-%m-%d")
        .map_err(|_| Refused::Date(partition.run_date.clone()))?;
    if !read_run_rows::<EvaluationSummaryRowV3>(root, &run_id)
        .await?
        .is_empty()
    {
        return Err(Refused::AlreadyMigrated(run_id));
    }
    let befores = read_run_rows::<EvaluationSummaryRowV2>(root, &run_id).await?;
    let [before] = befores.as_slice() else {
        return Err(Refused::NotOneSummary(run_id, befores.len()));
    };
    let run = read_run_row(root, &run_id).await?;
    let bootstrap = Bootstrap {
        resamples: NonZeroUsize::new(run.resamples as usize)
            .ok_or_else(|| Refused::NoResamples(run_id.clone()))?,
        seed: Seed(run.bootstrap_seed),
    };
    let mut sessions = read_run_rows::<EvaluationSessionRowV2>(root, &run_id).await?;
    sessions.sort_by(|one, other| one.session_id.cmp(&other.session_id));
    let outcomes = read_run_rows::<EvaluationOutcomeRowV2>(root, &run_id).await?;

    let again = steps::summarise(&sessions, &outcomes, &bootstrap)?;
    if !unchanged(before, &again) {
        return Err(Refused::Changed {
            run_id,
            before: Box::new(before.clone()),
            again: Box::new(again),
        });
    }
    write_run(root, date, &run_id, std::slice::from_ref(&again)).await?;
    tracing::info!(%run_id, %date, f_half = ?again.f_half, "migrated");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "evaluation_migrate_f_half=info".into()),
        )
        .init();
    let root = Args::parse().medallion.root()?;
    for partition in partitions(&root, EvaluationSummaryRowV2::DATASET).await? {
        migrate(&root, partition).await?;
    }
    Ok(())
}
