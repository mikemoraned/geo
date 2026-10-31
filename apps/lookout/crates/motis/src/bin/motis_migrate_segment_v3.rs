use std::collections::HashMap;

use chrono::{NaiveDate, Utc};
use clap::Parser;
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

use medallion::{MedallionArgs, Query, QueryError, Root};
use medallion_model::{
    MOTIS_POLYLINE_V1, MOTIS_SEGMENT_V2, MOTIS_SEGMENT_V3, MOTIS_SOURCE_V3, MOTIS_SOURCE_V4,
    MotisPollRowV1, MotisSourceId,
};
use motis::landing_to_bronze_promotion::{PollsWritten, PromotionError, write_polls};

const NEW_SOURCE_IDS: &str = "
    SELECT DISTINCT old.source_id AS old_id, new.source_id AS new_id
    FROM motis_source_v3 AS old
    JOIN motis_source_v4 AS new
      ON (old.base_url IS NOT DISTINCT FROM new.base_url)
     AND (old.motis_version IS NOT DISTINCT FROM new.motis_version)
     AND (old.feed IS NOT DISTINCT FROM new.feed)
     AND (old.country IS NOT DISTINCT FROM new.country)
     AND (old.overture_region_id IS NOT DISTINCT FROM new.overture_region_id)
     AND (old.overture_region_name IS NOT DISTINCT FROM new.overture_region_name)
     AND (old.overture_release IS NOT DISTINCT FROM new.overture_release)
     AND (old.label IS NOT DISTINCT FROM new.label)
     AND (old.registered_at IS NOT DISTINCT FROM new.registered_at)";

#[derive(Parser)]
#[command(about = "Copy bronze motis_segment_v2 into motis_segment_v3 and motis_polyline_v1")]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[derive(Debug, thiserror::Error)]
enum Refused {
    #[error("motis_segment_v3 or motis_polyline_v1 already holds rows")]
    AlreadyMigrated,
    #[error("the source {old} takes both of the new ids {first} and {second}")]
    TwoNewIds {
        old: MotisSourceId,
        first: MotisSourceId,
        second: MotisSourceId,
    },
    #[error("a segment of {date} names the source {old}, which motis_source_v3 lacks")]
    SourceMissing { date: NaiveDate, old: MotisSourceId },
    #[error(transparent)]
    Query(#[from] QueryError),
    #[error(transparent)]
    Path(#[from] medallion::PathError),
    #[error(transparent)]
    Promotion(#[from] PromotionError),
}

async fn new_source_ids(root: &Root) -> Result<HashMap<MotisSourceId, MotisSourceId>, Refused> {
    #[derive(Deserialize)]
    struct Pair {
        old_id: MotisSourceId,
        new_id: MotisSourceId,
    }

    let query = Query::new(root.clone());
    query.register_by_name(MOTIS_SOURCE_V3).await?;
    query.register_by_name(MOTIS_SOURCE_V4).await?;
    let pairs: Vec<Pair> = query.rows(NEW_SOURCE_IDS).await?;

    let mut new_ids = HashMap::new();
    for Pair { old_id, new_id } in pairs {
        if let Some(first) = new_ids.insert(old_id.clone(), new_id.clone()) {
            return Err(Refused::TwoNewIds {
                old: old_id,
                first,
                second: new_id,
            });
        }
    }
    Ok(new_ids)
}

async fn dates(root: &Root) -> Result<Vec<NaiveDate>, QueryError> {
    #[derive(Deserialize)]
    struct Day {
        date: NaiveDate,
    }

    let query = Query::new(root.clone());
    query.register_by_name(MOTIS_SEGMENT_V2).await?;
    let days: Vec<Day> = query
        .rows(
            "SELECT DISTINCT CAST(polled_date AS VARCHAR) AS date FROM motis_segment_v2
             ORDER BY date",
        )
        .await?;
    Ok(days.into_iter().map(|day| day.date).collect())
}

async fn polls_of(
    root: &Root,
    date: NaiveDate,
    new_ids: &HashMap<MotisSourceId, MotisSourceId>,
) -> Result<Vec<MotisPollRowV1>, Refused> {
    let query = Query::new(root.clone());
    query
        .register_at(&root.dataset(MOTIS_SEGMENT_V2).on_date(date)?, "day")
        .await?;
    let rows: Vec<MotisPollRowV1> = query.rows("SELECT * FROM day").await?;
    rows.into_iter()
        .map(|row| match new_ids.get(&row.source_id) {
            Some(new_id) => Ok(MotisPollRowV1 {
                source_id: new_id.clone(),
                ..row
            }),
            None => Err(Refused::SourceMissing {
                date,
                old: row.source_id,
            }),
        })
        .collect()
}

async fn migrate(root: &Root) -> Result<PollsWritten, Refused> {
    if root.dataset(MOTIS_SEGMENT_V3).is_filled() || root.dataset(MOTIS_POLYLINE_V1).is_filled() {
        return Err(Refused::AlreadyMigrated);
    }

    let new_ids = new_source_ids(root).await?;
    let mut migrated = PollsWritten::default();
    for date in dates(root).await? {
        let polls = polls_of(root, date, &new_ids).await?;
        let written = write_polls(root, Utc::now(), &polls).await?;
        tracing::info!(
            %date,
            segments = written.segments,
            polylines = written.polylines,
            "migrated a date"
        );
        migrated.segments += written.segments;
        migrated.polylines += written.polylines;
    }
    Ok(migrated)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "motis_migrate_segment_v3=info".into()),
        )
        .init();

    let root = Args::parse().medallion.root()?;
    let migrated = migrate(&root).await?;
    tracing::info!(
        segments = migrated.segments,
        polylines = migrated.polylines,
        "wrote motis_segment_v3 and motis_polyline_v1"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeZone};
    use medallion::Row;
    use medallion_model::{MotisCaptureRow, MotisSegmentRowV3, MotisSourceRowV3, MotisSourceRowV4};
    use serde_json::json;

    use super::*;

    const OLD_ID: &str = "dbc79993-2a04-51e0-a6f4-245ab1dfe468";
    const NEW_ID: &str = "70045828-c168-5bb0-ba31-0a273498bf80";

    fn old_id() -> MotisSourceId {
        serde_json::from_value(json!(OLD_ID)).unwrap()
    }

    fn new_id() -> MotisSourceId {
        serde_json::from_value(json!(NEW_ID)).unwrap()
    }

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, 8, 0, 0).unwrap()
    }

    fn source_fields(source_id: &str) -> serde_json::Value {
        json!({
            "source_id": source_id,
            "base_url": "https://api.transitous.org/",
            "motis_version": "v2.11.3",
            "feed": "transitous",
            "country": "DE",
            "overture_region_id": "e0ff5492-5758-4335-aabd-af1747925cd1",
            "overture_region_name": "Thüringen",
            "overture_release": "2026-09-23.1",
            "label": null,
            "registered_at": 0,
        })
    }

    async fn register_sources(root: &Root) {
        let old: MotisSourceRowV3 = serde_json::from_value(source_fields(OLD_ID)).unwrap();
        let new: MotisSourceRowV4 = serde_json::from_value(source_fields(NEW_ID)).unwrap();
        root.rows_of::<MotisSourceRowV3>()
            .append_rows(at(1), &[old])
            .await
            .unwrap();
        root.rows_of::<MotisSourceRowV4>()
            .append_rows(at(1), &[new])
            .await
            .unwrap();
    }

    async fn capture(root: &Root, source_id: MotisSourceId, captured_at: DateTime<Utc>) {
        let row = MotisCaptureRow {
            source_id,
            captured_at,
            ..MotisCaptureRow::example()
        };
        root.rows_of::<MotisCaptureRow>()
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
    async fn every_date_is_copied_under_each_segments_new_source_id() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        register_sources(&root).await;
        capture(&root, old_id(), at(1)).await;
        capture(&root, old_id(), at(2)).await;

        let migrated = migrate(&root).await.unwrap();

        assert_eq!(migrated.segments, 2);
        assert_eq!(migrated.polylines, 1);
        let segments = segments(&root).await;
        assert_eq!(
            segments
                .iter()
                .map(|row| row.captured_at)
                .collect::<Vec<_>>(),
            [at(1), at(2)]
        );
        assert!(segments.iter().all(|row| row.source_id == new_id()));
    }

    #[tokio::test]
    async fn a_segment_whose_source_is_missing_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        register_sources(&root).await;
        let unknown: MotisSourceId =
            serde_json::from_value(json!("00000000-0000-5000-8000-000000000000")).unwrap();
        capture(&root, unknown, at(1)).await;

        let err = migrate(&root).await.unwrap_err();

        assert!(matches!(err, Refused::SourceMissing { .. }), "{err}");
    }

    #[tokio::test]
    async fn a_second_run_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        register_sources(&root).await;
        capture(&root, old_id(), at(1)).await;
        migrate(&root).await.unwrap();

        let err = migrate(&root).await.unwrap_err();

        assert!(matches!(err, Refused::AlreadyMigrated), "{err}");
    }
}
