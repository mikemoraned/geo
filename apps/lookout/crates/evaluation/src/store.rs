use std::collections::BTreeMap;

use crossings::silver::Crossing;
use datafusion::prelude::{Expr, col, lit};
use domain::{CoordinateError, CrossingCompact, CrossingCompactId, CrossingId, Precision};
use medallion::{Query, QueryError, Root};
use medallion_model::{EvaluationSessionRowV1, TrainSessionCrossingRowV1};

pub async fn read_passes(
    root: &Root,
    sessions: &[EvaluationSessionRowV1],
) -> Result<Vec<TrainSessionCrossingRowV1>, QueryError> {
    Query::new(root.clone())
        .read::<TrainSessionCrossingRowV1>()
        .await?
        .filter(picked(sessions))?
        .rows()
        .await
}

pub(crate) fn picked(sessions: &[EvaluationSessionRowV1]) -> Expr {
    col("session_id").in_list(
        sessions
            .iter()
            .map(|session| lit(session.session_id.to_string()))
            .collect(),
        false,
    )
}

pub fn compact_ids(crossings: &[Crossing]) -> BTreeMap<CrossingId, CrossingCompactId> {
    crossings
        .iter()
        .map(|crossing| (crossing.crossing.id.clone(), crossing.compact_id))
        .collect()
}

pub fn compacted<F: Precision>(
    crossings: &[Crossing],
) -> Result<Vec<CrossingCompact<F>>, CoordinateError> {
    crossings
        .iter()
        .map(|crossing| {
            CrossingCompact::at(
                crossing.compact_id,
                crossing.crossing.latitude(),
                crossing.crossing.longitude(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::{DateTime, TimeDelta, Utc};
    use domain::SessionId;
    use medallion::{Row, write_rows};

    use super::*;

    fn instant(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
            + TimeDelta::seconds(seconds)
    }

    fn session(name: &str) -> SessionId {
        SessionId::new(format!("session-{name}")).expect("a session id")
    }

    fn picked_sessions(names: &[&str]) -> Vec<EvaluationSessionRowV1> {
        names
            .iter()
            .map(|&name| EvaluationSessionRowV1 {
                session_id: session(name),
                ..EvaluationSessionRowV1::example()
            })
            .collect()
    }

    fn pass(name: &str, seconds: i64) -> TrainSessionCrossingRowV1 {
        TrainSessionCrossingRowV1 {
            session_id: session(name),
            crossing_id: CrossingId::new("crossing-1").expect("a crossing id"),
            crossed_at: instant(seconds),
            ..TrainSessionCrossingRowV1::example()
        }
    }

    async fn store(dir: &Path) -> Root {
        let root = Root::new(dir);
        write_rows(&root, &[pass("a", 10), pass("b", 20), pass("c", 30)])
            .await
            .expect("write the passes");
        root
    }

    #[tokio::test]
    async fn only_the_picked_sessions_passes_are_read() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = store(tmp.path()).await;

        let mut passes = read_passes(&root, &picked_sessions(&["a", "c"]))
            .await
            .expect("the passes");
        passes.sort_by_key(|pass| pass.crossed_at);

        assert_eq!(passes, [pass("a", 10), pass("c", 30)]);
    }
}
