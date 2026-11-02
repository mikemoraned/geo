use chrono::{DateTime, Utc};
use clap::Parser;
use tracing_subscriber::EnvFilter;

use medallion::{MedallionArgs, Query, QueryError, Root};
use medallion_model::{
    MOTIS_SEGMENT, MOTIS_SEGMENT_V3, MotisPollRowV2, MotisSegmentRow, MotisSourceId,
};
use motis::landing_to_bronze_promotion::{PollsWritten, PromotionError, write_polls};
use motis::legacy::{legacy_source_id_sql, legacy_sources};

#[derive(Parser)]
#[command(about = "Copy bronze motis_segment into motis_segment_v3 and motis_polyline_v2")]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[derive(Debug, thiserror::Error)]
enum Refused {
    #[error("motis_segment_v3 already holds rows of a source of motis_segment")]
    AlreadyMigrated,
    #[error("trip `{trip_id}`, captured at {captured_at}, holds `{polyline}`, no encoded polyline")]
    NotAPolyline {
        trip_id: String,
        captured_at: DateTime<Utc>,
        polyline: String,
    },
    #[error(transparent)]
    Query(#[from] QueryError),
    #[error(transparent)]
    Promotion(#[from] PromotionError),
}

async fn already_migrated(root: &Root) -> Result<bool, QueryError> {
    let query = Query::new(root.clone());
    if !query
        .register_if_present(MOTIS_SEGMENT_V3, MOTIS_SEGMENT_V3.name)
        .await?
    {
        return Ok(false);
    }
    let [gtfs_de, delfi] = legacy_sources().map(|source| source.id());
    let held = query
        .count(&format!(
            "SELECT COUNT(*) AS count FROM {}
             WHERE source_id IN ('{gtfs_de}', '{delfi}')",
            MOTIS_SEGMENT_V3.name
        ))
        .await?;
    Ok(held > 0)
}

fn poll_of(source_id: MotisSourceId, row: MotisSegmentRow) -> Result<MotisPollRowV2, Refused> {
    let (trip_id, captured_at, polyline) =
        (row.trip_id.clone(), row.captured_at, row.polyline.clone());
    MotisPollRowV2::of(source_id, row).map_err(|_| Refused::NotAPolyline {
        trip_id,
        captured_at,
        polyline,
    })
}

async fn polls(root: &Root) -> Result<Vec<MotisPollRowV2>, Refused> {
    let query = Query::new(root.clone());
    query.register_by_name(MOTIS_SEGMENT).await?;
    let source_of_a_row = legacy_source_id_sql("captured_at");
    let mut polls = Vec::new();
    for source in legacy_sources() {
        let source_id = source.id();
        let rows: Vec<MotisSegmentRow> = query
            .rows(&format!(
                "SELECT * FROM {} WHERE {source_of_a_row} = '{source_id}'",
                MOTIS_SEGMENT.name
            ))
            .await?;
        for row in rows {
            polls.push(poll_of(source_id.clone(), row)?);
        }
    }
    Ok(polls)
}

async fn migrate(root: &Root) -> Result<PollsWritten, Refused> {
    if already_migrated(root).await? {
        return Err(Refused::AlreadyMigrated);
    }
    Ok(write_polls(root, Utc::now(), &polls(root).await?).await?)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "motis_migrate_segment=info".into()),
        )
        .init();

    let root = Args::parse().medallion.root()?;
    let migrated = migrate(&root).await?;
    tracing::info!(
        segments = migrated.segments,
        polylines = migrated.polylines,
        "wrote motis_segment_v3 and motis_polyline_v2"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use medallion::Row;
    use medallion_model::MotisSegmentRowV3;

    use super::*;

    fn on(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, day, 8, 0, 0).unwrap()
    }

    async fn capture(root: &Root, captured_at: DateTime<Utc>, polyline: &str) {
        let row = MotisSegmentRow {
            captured_at,
            polyline: polyline.to_string(),
            ..MotisSegmentRow::example()
        };
        root.rows_of::<MotisSegmentRow>()
            .on_date(captured_at.date_naive())
            .unwrap()
            .append_rows(captured_at, &[row])
            .await
            .unwrap();
    }

    async fn segments(root: &Root) -> Vec<MotisSegmentRowV3> {
        let query = Query::new(root.clone());
        query.register_by_name(MOTIS_SEGMENT_V3).await.unwrap();
        query
            .rows("SELECT * FROM motis_segment_v3 ORDER BY captured_at")
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn every_row_is_copied_under_the_source_its_capture_date_names() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        capture(&root, on(22), "_ibE_seK").await;
        capture(&root, on(23), "_ibE_seK").await;

        let migrated = migrate(&root).await.unwrap();

        assert_eq!((migrated.segments, migrated.polylines), (2, 1));
        let [gtfs_de, delfi] = legacy_sources().map(|source| source.id());
        let copied: Vec<(DateTime<Utc>, MotisSourceId)> = segments(&root)
            .await
            .into_iter()
            .map(|row| (row.captured_at, row.source_id))
            .collect();
        assert_eq!(copied, [(on(22), gtfs_de), (on(23), delfi)]);
    }

    #[tokio::test]
    async fn a_row_holding_no_encoded_polyline_is_refused_and_nothing_is_written() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        capture(&root, on(22), "_ibE_seK").await;
        capture(&root, on(23), "not a polyline").await;

        let err = migrate(&root).await.unwrap_err();

        assert!(
            matches!(&err, Refused::NotAPolyline { polyline, .. } if polyline == "not a polyline"),
            "{err}"
        );
        assert!(!root.dataset(MOTIS_SEGMENT_V3).is_filled());
    }

    #[tokio::test]
    async fn a_second_run_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        capture(&root, on(22), "_ibE_seK").await;
        migrate(&root).await.unwrap();

        let err = migrate(&root).await.unwrap_err();

        assert!(matches!(err, Refused::AlreadyMigrated), "{err}");
    }
}
