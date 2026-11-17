use std::collections::BTreeMap;

use crossings::silver::Crossing;
use datafusion::prelude::{Expr, col, lit};
use domain::{CrossingCompactId, CrossingId, SessionId};
use itertools::Itertools;
use medallion::{Country, Query, QueryError, Root};
use medallion_model::{
    TrainSessionCrossingRowV1, TrainSessionSamplePositionV3, TrainSessionSampleRowV3,
};

pub async fn read_passes<'a>(
    root: &Root,
    session_ids: impl IntoIterator<Item = &'a SessionId>,
) -> Result<Vec<TrainSessionCrossingRowV1>, QueryError> {
    Query::new(root.clone())
        .read::<TrainSessionCrossingRowV1>()
        .await?
        .filter(picked(session_ids))?
        .rows()
        .await
}

pub async fn read_samples<'a>(
    root: &Root,
    country: Country,
    session_ids: impl IntoIterator<Item = &'a SessionId>,
) -> Result<BTreeMap<SessionId, Vec<TrainSessionSamplePositionV3>>, QueryError> {
    let rows = Query::new(root.clone())
        .read_of_country::<TrainSessionSampleRowV3>(country)
        .await?
        .filter(picked(session_ids))?
        .sort(vec![
            col("session_id").sort(true, false),
            col("t").sort(true, false),
            col("seq").sort(true, false),
        ])?
        .rows_as::<TrainSessionSamplePositionV3>()
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.session_id.clone(), row))
        .into_group_map()
        .into_iter()
        .collect())
}

fn picked<'a>(session_ids: impl IntoIterator<Item = &'a SessionId>) -> Expr {
    col("session_id").in_list(
        session_ids
            .into_iter()
            .map(|session_id| lit(session_id.to_string()))
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use domain::SessionId;
    use medallion::{Row, write_rows};

    use super::*;
    use crate::fixtures::instant;

    fn session(name: &str) -> SessionId {
        SessionId::new(format!("session-{name}")).expect("a session id")
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

        let mut passes = read_passes(&root, [&session("a"), &session("c")])
            .await
            .expect("the passes");
        passes.sort_by_key(|pass| pass.crossed_at);

        assert_eq!(passes, [pass("a", 10), pass("c", 30)]);
    }
}
