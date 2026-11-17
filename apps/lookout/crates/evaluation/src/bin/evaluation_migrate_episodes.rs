use chrono::NaiveDate;
use clap::Parser;
use domain::{CrossingCompactId, SessionId};
use evaluation::record::in_route_order;
use evaluation::runs::{RunsError, read_run_rows};
use evaluation::{
    CompactError, EpisodeError, compact_outcome_rows, episode_rows, outcome_rows_of, read_passes,
    transition_rows_of,
};
use itertools::Itertools;
use medallion::{MedallionArgs, PathError, Query, QueryError, Root, RunError, RunId, write_run};
use medallion_model::{
    EVALUATION_SESSION_V2, EvaluationEpisodeRowV1, EvaluationOutcomeRowV2, EvaluationOutcomeRowV3,
    EvaluationSessionRowV2, EvaluationSessionRowV3, EvaluationTransitionRowV1,
};
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    about = "Copy each run of gold evaluation_session_v2, evaluation_transition_v1, and \
             evaluation_outcome_v2 into evaluation_session_v3, evaluation_episode_v1, and \
             evaluation_outcome_v3, under its id, numbering its sessions in route order from \
             silver's passes, and refusing a run whose new rows do not rebuild its old ones"
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
    #[error("the {dataset} of run {run_id} do not rebuild its old rows")]
    Lossy {
        dataset: &'static str,
        run_id: RunId,
    },
    #[error("run date `{0}` is no date")]
    Date(String),
    #[error(transparent)]
    Episode(#[from] EpisodeError),
    #[error(transparent)]
    Compact(#[from] CompactError),
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
    query.register_gold(EVALUATION_SESSION_V2).await?;
    query
        .rows(&format!(
            "SELECT DISTINCT CAST(run_date AS VARCHAR) AS run_date, run_id FROM {} ORDER BY run_id",
            EVALUATION_SESSION_V2.name
        ))
        .await
}

async fn sessions(
    root: &Root,
    old: &[EvaluationSessionRowV2],
) -> Result<Vec<EvaluationSessionRowV3>, Refused> {
    let passes = read_passes(root, old.iter().map(|session| &session.session_id)).await?;
    Ok(in_route_order(old, |session| &session.session_id, &passes)
        .map(|(session_index, session)| EvaluationSessionRowV3 {
            session_index,
            session_id: session.session_id.clone(),
            started_at: session.started_at,
            ended_at: session.ended_at,
            duration: session.duration,
            scored_duration: session.scored_duration,
        })
        .collect())
}

fn transition_key(
    row: &EvaluationTransitionRowV1,
) -> (SessionId, chrono::DateTime<chrono::Utc>, CrossingCompactId) {
    (row.session_id.clone(), row.at, row.crossing_compact_id)
}

fn outcome_key(
    row: &EvaluationOutcomeRowV2,
) -> (
    SessionId,
    CrossingCompactId,
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
) {
    (
        row.session_id.clone(),
        row.crossing_compact_id,
        row.notified_at,
        row.crossed_at,
    )
}

async fn refuse_migrated<New: medallion::Row<Layer = medallion::layers::Gold>>(
    root: &Root,
    run_id: &RunId,
) -> Result<(), Refused> {
    if read_run_rows::<New>(root, run_id).await?.is_empty() {
        Ok(())
    } else {
        Err(Refused::AlreadyMigrated {
            dataset: New::DATASET.name,
            run_id: run_id.clone(),
        })
    }
}

async fn migrate(root: &Root, partition: Partition) -> Result<(), Refused> {
    let run_id = RunId::new(partition.run_id)?;
    let date = NaiveDate::parse_from_str(&partition.run_date, "%Y-%m-%d")
        .map_err(|_| Refused::Date(partition.run_date.clone()))?;
    refuse_migrated::<EvaluationSessionRowV3>(root, &run_id).await?;
    refuse_migrated::<EvaluationEpisodeRowV1>(root, &run_id).await?;
    refuse_migrated::<EvaluationOutcomeRowV3>(root, &run_id).await?;

    let sessions = sessions(
        root,
        &read_run_rows::<EvaluationSessionRowV2>(root, &run_id).await?,
    )
    .await?;

    let transitions: Vec<EvaluationTransitionRowV1> =
        read_run_rows::<EvaluationTransitionRowV1>(root, &run_id)
            .await?
            .into_iter()
            .sorted_by_key(transition_key)
            .collect();
    let episodes = episode_rows(&sessions, &transitions)?;
    if transition_rows_of(&sessions, &episodes)? != transitions {
        return Err(Refused::Lossy {
            dataset: "episodes",
            run_id,
        });
    }

    let outcomes: Vec<EvaluationOutcomeRowV2> =
        read_run_rows::<EvaluationOutcomeRowV2>(root, &run_id)
            .await?
            .into_iter()
            .sorted_by_key(outcome_key)
            .collect();
    let compact = compact_outcome_rows(&sessions, &outcomes)?;
    if outcome_rows_of(&sessions, &compact)? != outcomes {
        return Err(Refused::Lossy {
            dataset: "compact outcomes",
            run_id,
        });
    }

    write_run(root, date, &run_id, &sessions).await?;
    write_run(root, date, &run_id, &episodes).await?;
    write_run(root, date, &run_id, &compact).await?;
    tracing::info!(
        %run_id,
        %date,
        sessions = sessions.len(),
        transitions = transitions.len(),
        episodes = episodes.len(),
        outcomes = compact.len(),
        "migrated"
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "evaluation_migrate_episodes=info".into()),
        )
        .init();
    let root = Args::parse().medallion.root()?;
    for partition in partitions(&root).await? {
        migrate(&root, partition).await?;
    }
    Ok(())
}
