use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, NaiveDate, Utc};
use medallion::{AppendError, LandingError, PathError, Query, QueryError, Root};
use medallion_model::{
    MOTIS_POLL_V1, MOTIS_POLYLINE_V1, MOTIS_SEGMENT_V3, MotisPollRowV1, MotisPolylineRowV1,
    MotisSegmentRowV3, MotisSourceId, PolylineId,
};
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum PromotionError {
    #[error(transparent)]
    Landing(#[from] LandingError),
    #[error(transparent)]
    Query(#[from] QueryError),
    #[error(transparent)]
    Path(#[from] PathError),
    #[error(transparent)]
    Append(#[from] AppendError),
    #[error(
        "bronze holds {held} rows of {source_id}'s poll at {captured_at}, fewer than the {landed} \
         landing holds, so landing keeps its files"
    )]
    PollNotHeld {
        source_id: MotisSourceId,
        captured_at: DateTime<Utc>,
        held: usize,
        landed: usize,
    },
    #[error("bronze lacks the polyline {0}, so landing keeps its files")]
    PolylineNotHeld(PolylineId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Promoted {
    pub files: usize,
    pub segments: usize,
    pub polylines: usize,
}

type Poll = (MotisSourceId, DateTime<Utc>);

fn poll_of(row: &MotisPollRowV1) -> Poll {
    (row.source_id.clone(), row.captured_at)
}

pub async fn promote(root: &Root, at: DateTime<Utc>) -> Result<Promoted, PromotionError> {
    let files = root.rows_of::<MotisPollRowV1>().landed_files()?;
    if files.is_empty() {
        return Ok(Promoted::default());
    }

    let landed = landed_rows(root).await?;
    let polylines = append_polylines(root, at, &landed).await?;
    let segments = append_segments(root, at, &landed).await?;
    check_bronze_holds(root, &landed).await?;

    let promoted = Promoted {
        files: files.len(),
        segments,
        polylines,
    };
    for file in files {
        file.delete()?;
    }
    Ok(promoted)
}

async fn landed_rows(root: &Root) -> Result<Vec<MotisPollRowV1>, QueryError> {
    let query = Query::new(root.clone());
    query.register_by_name(MOTIS_POLL_V1).await?;
    query
        .rows(&format!("SELECT * FROM {}", MOTIS_POLL_V1.name))
        .await
}

async fn polls_held(root: &Root) -> Result<HashMap<Poll, usize>, QueryError> {
    #[derive(Deserialize)]
    struct Held {
        source_id: MotisSourceId,
        #[serde(with = "chrono::serde::ts_milliseconds")]
        captured_at: DateTime<Utc>,
        row_count: u64,
    }

    let query = Query::new(root.clone());
    if !query
        .register_if_present(MOTIS_SEGMENT_V3, MOTIS_SEGMENT_V3.name)
        .await?
    {
        return Ok(HashMap::new());
    }
    let held: Vec<Held> = query
        .rows(&format!(
            "SELECT source_id, captured_at, COUNT(*) AS row_count FROM {} \
             GROUP BY source_id, captured_at",
            MOTIS_SEGMENT_V3.name
        ))
        .await?;
    Ok(held
        .into_iter()
        .map(|held| {
            (
                (held.source_id, held.captured_at),
                usize::try_from(held.row_count).expect("a row count fits in memory"),
            )
        })
        .collect())
}

async fn polylines_held(root: &Root) -> Result<BTreeSet<PolylineId>, QueryError> {
    #[derive(Deserialize)]
    struct Held {
        polyline_id: PolylineId,
    }

    let query = Query::new(root.clone());
    if !query
        .register_if_present(MOTIS_POLYLINE_V1, MOTIS_POLYLINE_V1.name)
        .await?
    {
        return Ok(BTreeSet::new());
    }
    let held: Vec<Held> = query
        .rows(&format!(
            "SELECT polyline_id FROM {}",
            MOTIS_POLYLINE_V1.name
        ))
        .await?;
    Ok(held.into_iter().map(|held| held.polyline_id).collect())
}

async fn append_polylines(
    root: &Root,
    at: DateTime<Utc>,
    landed: &[MotisPollRowV1],
) -> Result<usize, PromotionError> {
    let held = polylines_held(root).await?;
    let lacking: BTreeMap<PolylineId, MotisPolylineRowV1> = landed
        .iter()
        .map(|row| MotisPolylineRowV1::of(row.polyline.clone()))
        .filter(|row| !held.contains(&row.polyline_id))
        .map(|row| (row.polyline_id.clone(), row))
        .collect();
    let rows: Vec<MotisPolylineRowV1> = lacking.into_values().collect();
    Ok(root
        .rows_of::<MotisPolylineRowV1>()
        .append_rows(at, &rows)
        .await?)
}

async fn append_segments(
    root: &Root,
    at: DateTime<Utc>,
    landed: &[MotisPollRowV1],
) -> Result<usize, PromotionError> {
    let mut by_date: BTreeMap<NaiveDate, Vec<MotisSegmentRowV3>> = BTreeMap::new();
    for row in landed {
        by_date
            .entry(row.captured_at.date_naive())
            .or_default()
            .push(MotisSegmentRowV3::of(row.clone()));
    }

    let mut written = 0;
    for (date, mut rows) in by_date {
        rows.sort_by(|a, b| storage_order(a).cmp(&storage_order(b)));
        written += root
            .rows_of::<MotisSegmentRowV3>()
            .on_date(date)?
            .append_rows(at, &rows)
            .await?;
    }
    Ok(written)
}

fn storage_order(
    row: &MotisSegmentRowV3,
) -> (
    &Option<String>,
    &Option<String>,
    &PolylineId,
    &str,
    DateTime<Utc>,
) {
    (
        &row.from_stop_id,
        &row.to_stop_id,
        &row.polyline_id,
        &row.trip_id,
        row.captured_at,
    )
}

async fn check_bronze_holds(root: &Root, landed: &[MotisPollRowV1]) -> Result<(), PromotionError> {
    let held = polls_held(root).await?;
    let mut landed_counts: HashMap<Poll, usize> = HashMap::new();
    for row in landed {
        *landed_counts.entry(poll_of(row)).or_default() += 1;
    }
    for ((source_id, captured_at), landed) in landed_counts {
        let held = held
            .get(&(source_id.clone(), captured_at))
            .copied()
            .unwrap_or_default();
        if held < landed {
            return Err(PromotionError::PollNotHeld {
                source_id,
                captured_at,
                held,
                landed,
            });
        }
    }

    let polylines = polylines_held(root).await?;
    landed
        .iter()
        .map(|row| PolylineId::of(&row.polyline))
        .find(|id| !polylines.contains(id))
        .map_or(Ok(()), |id| Err(PromotionError::PolylineNotHeld(id)))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap as Map;

    use chrono::TimeZone;
    use medallion_model::{Feed, Polyline};

    use super::*;
    use crate::api::types::TripSegment;
    use crate::landing::SegmentLog;
    use crate::segment::SingleTripSegment;
    use crate::testing::{regional_trips, source};

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 1, hour, 0, 0).unwrap()
    }

    fn one_trip_each(segments: Vec<TripSegment>) -> Vec<SingleTripSegment> {
        segments
            .into_iter()
            .map(|segment| SingleTripSegment::new(segment).expect("a segment with one trip"))
            .collect()
    }

    async fn land(root: &Root, feed: Feed, captured_at: DateTime<Utc>) -> usize {
        SegmentLog::new(root.clone(), source(feed).id())
            .append(captured_at, &one_trip_each(regional_trips()), &Map::new())
            .await
            .expect("land a poll")
    }

    async fn segments(root: &Root) -> Vec<MotisSegmentRowV3> {
        let query = Query::new(root.clone());
        if !query
            .register_if_present(MOTIS_SEGMENT_V3, MOTIS_SEGMENT_V3.name)
            .await
            .expect("register")
        {
            return Vec::new();
        }
        query
            .rows("SELECT * FROM motis_segment_v3")
            .await
            .expect("read the segments")
    }

    async fn polylines(root: &Root) -> Vec<MotisPolylineRowV1> {
        let query = Query::new(root.clone());
        query
            .register_by_name(MOTIS_POLYLINE_V1)
            .await
            .expect("register");
        query
            .rows("SELECT * FROM motis_polyline_v1")
            .await
            .expect("read the polylines")
    }

    fn landed_files(root: &Root) -> usize {
        root.rows_of::<MotisPollRowV1>()
            .landed_files()
            .expect("list landing")
            .len()
    }

    #[tokio::test]
    async fn an_empty_landing_writes_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());

        let promoted = promote(&root, at(23)).await.expect("promote");

        assert_eq!(promoted, Promoted::default());
        assert!(segments(&root).await.is_empty());
    }

    #[tokio::test]
    async fn polls_move_from_landing_into_bronze() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let rows = land(&root, Feed::Local, at(8)).await;
        land(&root, Feed::Local, at(9)).await;

        let promoted = promote(&root, at(23)).await.expect("promote");

        assert_eq!(promoted.files, 2);
        assert_eq!(promoted.segments, rows * 2);
        assert_eq!(segments(&root).await.len(), rows * 2);
        assert_eq!(landed_files(&root), 0);
    }

    #[tokio::test]
    async fn a_segments_polyline_text_is_found_through_its_id() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        land(&root, Feed::Local, at(8)).await;

        promote(&root, at(23)).await.expect("promote");

        let texts: Map<PolylineId, Polyline> = polylines(&root)
            .await
            .into_iter()
            .map(|row| (row.polyline_id, row.polyline))
            .collect();
        let found: BTreeSet<String> = segments(&root)
            .await
            .iter()
            .map(|row| texts[&row.polyline_id].to_string())
            .collect();
        let expected: BTreeSet<String> = regional_trips()
            .into_iter()
            .map(|segment| segment.polyline)
            .collect();
        assert_eq!(found, expected);
    }

    #[tokio::test]
    async fn a_polyline_bronze_holds_is_not_written_again() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        land(&root, Feed::Local, at(8)).await;
        land(&root, Feed::Local, at(9)).await;

        let first = promote(&root, at(22)).await.expect("first promotion");
        land(&root, Feed::Local, at(10)).await;
        let second = promote(&root, at(23)).await.expect("second promotion");

        let distinct: BTreeSet<String> = regional_trips()
            .into_iter()
            .map(|segment| segment.polyline)
            .collect();
        assert_eq!(first.polylines, distinct.len());
        assert_eq!(second.polylines, 0);
        assert_eq!(polylines(&root).await.len(), distinct.len());
    }

    #[tokio::test]
    async fn each_date_takes_one_file_sorted_for_compression() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let next_day = Utc.with_ymd_and_hms(2026, 10, 2, 8, 0, 0).unwrap();
        land(&root, Feed::Local, at(8)).await;
        land(&root, Feed::Local, at(9)).await;
        land(&root, Feed::Local, next_day).await;

        promote(&root, at(23)).await.expect("promote");

        let day = root
            .rows_of::<MotisSegmentRowV3>()
            .on_date(at(8).date_naive())
            .unwrap();
        let files = std::fs::read_dir(day.dir()).unwrap().count();
        assert_eq!(files, 1);
        let query = Query::new(root.clone());
        query.register_at(&day, "day").await.expect("register");
        let rows: Vec<MotisSegmentRowV3> =
            query.rows("SELECT * FROM day").await.expect("read the day");
        assert!(
            rows.windows(2)
                .all(|pair| storage_order(&pair[0]) <= storage_order(&pair[1]))
        );
    }

    #[tokio::test]
    async fn a_poll_landing_again_is_held_twice_with_its_polylines_held_once() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let rows = land(&root, Feed::Local, at(8)).await;
        promote(&root, at(22)).await.expect("first promotion");
        land(&root, Feed::Local, at(8)).await;

        let second = promote(&root, at(23)).await.expect("second promotion");

        assert_eq!(second.segments, rows);
        assert_eq!(second.polylines, 0);
        assert_eq!(segments(&root).await.len(), rows * 2);
        assert_eq!(landed_files(&root), 0);
    }

    #[tokio::test]
    async fn bronze_holding_fewer_rows_of_a_poll_than_landing_fails_the_check() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        land(&root, Feed::Local, at(8)).await;
        let landed = landed_rows(&root).await.expect("read landing");
        append_polylines(&root, at(22), &landed)
            .await
            .expect("write the polylines");
        append_segments(&root, at(22), &landed[..1])
            .await
            .expect("write part of the poll");

        let err = check_bronze_holds(&root, &landed).await.unwrap_err();

        assert!(matches!(err, PromotionError::PollNotHeld { .. }), "{err}");
    }

    #[tokio::test]
    async fn bronze_lacking_a_polyline_fails_the_check() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        land(&root, Feed::Local, at(8)).await;
        let landed = landed_rows(&root).await.expect("read landing");
        append_segments(&root, at(22), &landed)
            .await
            .expect("write the segments");

        let err = check_bronze_holds(&root, &landed).await.unwrap_err();

        assert!(matches!(err, PromotionError::PolylineNotHeld(_)), "{err}");
    }
}
