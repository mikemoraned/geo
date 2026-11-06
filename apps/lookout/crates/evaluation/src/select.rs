use std::num::NonZeroUsize;

use datafusion::prelude::{Expr, col, concat, lit, sha256};
use domain::Percentage;
use medallion::{Country, Query, QueryError, Root};
use medallion_model::TrainSessionRowV3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Seed(pub u64);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Selection {
    pub country: Country,
    pub realtime_at_least: Percentage,
    pub sessions: NonZeroUsize,
    pub seed: Seed,
}

#[derive(Debug, thiserror::Error)]
pub enum SelectError {
    #[error("reading the train sessions: {0}")]
    Query(#[from] QueryError),
    #[error("{wanted} sessions were wanted, and only {eligible} pass the filter")]
    TooFew {
        wanted: NonZeroUsize,
        eligible: usize,
    },
}

pub async fn select(
    root: &Root,
    selection: &Selection,
) -> Result<Vec<TrainSessionRowV3>, SelectError> {
    let wanted = selection.sessions.get();
    let picked = Query::new(root.clone())
        .read_of_country::<TrainSessionRowV3>(selection.country)
        .await?
        .filter(col("realtime_percentage").gt_eq(lit(f64::from(selection.realtime_at_least))))?
        .sort(vec![
            draw_order(selection.seed).sort(true, false),
            col("session_id").sort(true, false),
        ])?
        .limit(wanted)?
        .sort(vec![col("session_id").sort(true, false)])?
        .rows()
        .await?;
    if picked.len() < wanted {
        return Err(SelectError::TooFew {
            wanted: selection.sessions,
            eligible: picked.len(),
        });
    }
    Ok(picked)
}

fn draw_order(seed: Seed) -> Expr {
    sha256(concat(vec![lit(format!("{}:", seed.0)), col("session_id")]))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::{DateTime, TimeDelta, Utc};
    use domain::SessionId;
    use geo_types::LineString;
    use medallion::{GeographyRow, Row, write_geography_rows};

    use super::*;

    fn started(day: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
            + TimeDelta::days(day)
    }

    fn session(n: usize, realtime: f64, day: i64) -> TrainSessionRowV3 {
        TrainSessionRowV3 {
            session_id: SessionId::new(format!("session-{n:05}")).expect("a session id"),
            started_at: started(day),
            ended_at: started(day),
            realtime_percentage: Percentage::new(realtime).expect("a percentage"),
            ..TrainSessionRowV3::example()
        }
    }

    fn sessions(count: usize) -> Vec<TrainSessionRowV3> {
        (0..count).map(|n| session(n, 100.0, 0)).collect()
    }

    async fn stored(dir: &Path, sessions: Vec<TrainSessionRowV3>) -> Root {
        let root = Root::new(dir);
        let rows: Vec<GeographyRow<TrainSessionRowV3, LineString<f64>>> = sessions
            .into_iter()
            .map(|row| GeographyRow {
                row,
                geography: LineString::from(vec![(13.73, 51.04), (13.74, 51.05)]),
                country: Country::Germany,
            })
            .collect();
        write_geography_rows(&root, &rows)
            .await
            .expect("write the sessions");
        root
    }

    fn selection(sessions: usize, seed: u64) -> Selection {
        Selection {
            country: Country::Germany,
            realtime_at_least: Percentage::new(90.0).expect("a percentage"),
            sessions: NonZeroUsize::new(sessions).expect("not zero"),
            seed: Seed(seed),
        }
    }

    fn ids(sessions: &[TrainSessionRowV3]) -> Vec<String> {
        sessions
            .iter()
            .map(|session| String::from(session.session_id.clone()))
            .collect()
    }

    #[tokio::test]
    async fn it_picks_as_many_distinct_sessions_as_asked_in_order_of_id() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path(), sessions(100)).await;

        let picked = select(&root, &selection(10, 1))
            .await
            .expect("enough sessions");

        assert_eq!(picked.len(), 10);
        assert!(
            ids(&picked).is_sorted_by(|one, next| one < next),
            "{:?}",
            ids(&picked)
        );
    }

    #[tokio::test]
    async fn it_picks_only_sessions_at_or_above_the_realtime_percentage() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let candidates = (0..100)
            .map(|n| session(n, if n % 2 == 0 { 90.0 } else { 89.9 }, 0))
            .collect();
        let root = stored(tmp.path(), candidates).await;

        let picked = select(&root, &selection(50, 1))
            .await
            .expect("enough sessions");

        assert_eq!(picked.len(), 50);
        assert!(
            picked
                .iter()
                .all(|session| f64::from(session.realtime_percentage) >= 90.0)
        );
    }

    #[tokio::test]
    async fn it_refuses_a_selection_larger_than_the_sessions_that_pass_the_filter() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let candidates = (0..10)
            .map(|n| session(n, if n < 3 { 100.0 } else { 0.0 }, 0))
            .collect();
        let root = stored(tmp.path(), candidates).await;

        let refused = select(&root, &selection(4, 1)).await;

        assert!(
            matches!(
                refused,
                Err(SelectError::TooFew { wanted, eligible: 3 }) if wanted.get() == 4
            ),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn another_seed_picks_other_sessions() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path(), sessions(1_000)).await;

        let one = select(&root, &selection(10, 1))
            .await
            .expect("enough sessions");
        let other = select(&root, &selection(10, 2))
            .await
            .expect("enough sessions");

        assert_ne!(ids(&one), ids(&other));
    }

    #[tokio::test]
    async fn a_seed_picks_the_same_sessions_on_every_platform_and_every_run() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path(), sessions(1_000)).await;

        let picked = select(&root, &selection(3, 42))
            .await
            .expect("enough sessions");

        assert_eq!(
            ids(&picked),
            ["session-00326", "session-00706", "session-00784"]
        );
    }

    #[tokio::test]
    async fn the_days_the_sessions_started_on_leave_the_draw_unchanged() {
        let on_one_day = tempfile::tempdir().expect("a temporary directory");
        let over_a_week = tempfile::tempdir().expect("a temporary directory");
        let one_day = stored(on_one_day.path(), sessions(200)).await;
        let week = stored(
            over_a_week.path(),
            (0..200)
                .map(|n| session(n, 100.0, (n % 7) as i64))
                .collect(),
        )
        .await;

        let from_one_day = select(&one_day, &selection(20, 7))
            .await
            .expect("enough sessions");
        let from_a_week = select(&week, &selection(20, 7))
            .await
            .expect("enough sessions");

        assert_eq!(ids(&from_one_day), ids(&from_a_week));
    }

    #[tokio::test]
    async fn a_country_holding_no_train_sessions_is_refused() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path(), sessions(10)).await;

        let refused = select(
            &root,
            &Selection {
                country: Country::UnitedKingdom,
                ..selection(1, 1)
            },
        )
        .await;

        assert!(
            matches!(
                refused,
                Err(SelectError::Query(QueryError::NoSuchDataset { .. }))
            ),
            "{refused:?}"
        );
    }
}
