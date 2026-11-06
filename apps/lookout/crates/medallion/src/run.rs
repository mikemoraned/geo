use std::fmt::{self, Display};
use std::path::{Path, PathBuf};

use chrono::NaiveDate;

use crate::layer::layers;
use crate::partition::{Partition, PartitionValue, PathError};
use crate::path::Root;
use crate::rows::{Geometry, Row, RowError, batch};
use crate::write::WriteError;

pub const RUN_DATE: &str = "run_date";
pub const RUN_ID: &str = "run_id";

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RunId(String);

impl RunId {
    pub fn new(id: impl Into<String>) -> Result<Self, PathError> {
        let id = id.into();
        PartitionValue::new(id.clone())?;
        Ok(Self(id))
    }
}

impl Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("{dataset} is not laid out by run: its partition key is {key:?}, not `{RUN_DATE}`")]
    NotByRun {
        dataset: &'static str,
        key: Option<&'static str>,
    },
    #[error("{dataset} holds geometry, which a run's rows do not carry")]
    Geometry { dataset: &'static str },
    #[error("{dataset} already holds run {run_id}, written under {RUN_DATE}={written_on}")]
    Written {
        dataset: &'static str,
        run_id: RunId,
        written_on: String,
    },
    #[error(transparent)]
    Path(#[from] PathError),
    #[error(transparent)]
    Row(#[from] RowError),
    #[error(transparent)]
    Write(#[from] WriteError),
}

pub async fn write_run<T: Row<Layer = layers::Gold>>(
    root: &Root,
    date: NaiveDate,
    run_id: &RunId,
    rows: &[T],
) -> Result<PathBuf, RunError> {
    let dataset = T::DATASET;
    if dataset.partition_key != Some(RUN_DATE) {
        return Err(RunError::NotByRun {
            dataset: dataset.name,
            key: dataset.partition_key,
        });
    }
    if T::GEOMETRY != Geometry::Absent {
        return Err(RunError::Geometry {
            dataset: dataset.name,
        });
    }
    let runs = root.rows_of::<T>();
    if let Some(written_on) = written_on(&runs.dir(), run_id) {
        return Err(RunError::Written {
            dataset: dataset.name,
            run_id: run_id.clone(),
            written_on,
        });
    }
    let partition = runs.on_date(date)?.partition(RUN_ID, run_id)?;
    Ok(partition.replace_with(&[batch(rows)?]).await?)
}

fn written_on(runs: &Path, run_id: &RunId) -> Option<String> {
    let run = Partition::new(RUN_ID, run_id).ok()?.to_string();
    std::fs::read_dir(runs)
        .ok()?
        .flatten()
        .filter(|date| date.path().join(&run).is_dir())
        .find_map(|date| {
            date.file_name()
                .to_str()?
                .strip_prefix(&format!("{RUN_DATE}="))
                .map(str::to_string)
        })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::dataset::DatasetSpec;
    use crate::query::Query;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Scored {
        notification_id: String,
        score: f64,
    }

    impl Row for Scored {
        type Layer = layers::Gold;
        const DATASET: DatasetSpec<Self::Layer> = DatasetSpec::partitioned("scored", RUN_DATE);

        fn example() -> Self {
            Scored {
                notification_id: "example".to_string(),
                score: 1.0,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Exported {
        name: String,
    }

    impl Row for Exported {
        type Layer = layers::Gold;
        const DATASET: DatasetSpec<Self::Layer> = DatasetSpec::partitioned("exported", "made_date");

        fn example() -> Self {
            Exported {
                name: "example".to_string(),
            }
        }
    }

    fn scored(count: u32) -> Vec<Scored> {
        (1..=count)
            .map(|n| Scored {
                notification_id: format!("notification-{n}"),
                score: f64::from(n) / 10.0,
            })
            .collect()
    }

    fn day(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, day).expect("a date")
    }

    fn run(id: &str) -> RunId {
        RunId::new(id).expect("a run id")
    }

    async fn read_back(dir: &Path) -> Vec<Scored> {
        let mut rows = Query::new(Root::new(dir))
            .read::<Scored>()
            .await
            .expect("a read of the runs")
            .rows()
            .await
            .expect("the rows");
        rows.sort_by(|one, other| one.notification_id.cmp(&other.notification_id));
        rows
    }

    #[tokio::test]
    async fn a_runs_rows_are_written_under_its_date_and_id() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = Root::new(tmp.path());

        let path = write_run(&root, day(6), &run("abc"), &scored(3))
            .await
            .expect("a written run");

        assert!(
            path.starts_with(
                tmp.path()
                    .join("gold/scored/run_date=2026-11-06/run_id=abc")
            ),
            "{}",
            path.display()
        );
        assert_eq!(read_back(tmp.path()).await, scored(3));
    }

    #[tokio::test]
    async fn two_runs_are_kept_side_by_side() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = Root::new(tmp.path());

        write_run(&root, day(6), &run("abc"), &scored(1))
            .await
            .expect("a written run");
        write_run(&root, day(6), &run("def"), &scored(2))
            .await
            .expect("a written run");

        assert_eq!(read_back(tmp.path()).await.len(), 3);
    }

    #[tokio::test]
    async fn a_run_written_again_is_refused_and_its_rows_are_kept() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = Root::new(tmp.path());
        write_run(&root, day(6), &run("abc"), &scored(2))
            .await
            .expect("a written run");

        let refused = write_run(&root, day(6), &run("abc"), &scored(5)).await;

        assert!(
            matches!(
                &refused,
                Err(RunError::Written { run_id, written_on, .. })
                    if *run_id == run("abc") && written_on == "2026-11-06"
            ),
            "{refused:?}"
        );
        assert_eq!(read_back(tmp.path()).await, scored(2));
    }

    #[tokio::test]
    async fn a_run_written_again_on_another_date_is_refused() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = Root::new(tmp.path());
        write_run(&root, day(6), &run("abc"), &scored(2))
            .await
            .expect("a written run");

        let refused = write_run(&root, day(7), &run("abc"), &scored(2)).await;

        assert!(
            matches!(
                &refused,
                Err(RunError::Written { written_on, .. }) if written_on == "2026-11-06"
            ),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn a_run_with_no_rows_is_still_written() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = Root::new(tmp.path());

        let path = write_run(&root, day(6), &run("abc"), &Vec::<Scored>::new())
            .await
            .expect("a written run");

        assert!(path.is_file(), "{}", path.display());
        assert!(read_back(tmp.path()).await.is_empty());
    }

    #[tokio::test]
    async fn a_dataset_not_laid_out_by_run_is_refused() {
        let tmp = tempfile::tempdir().expect("a temporary directory");

        let refused = write_run(
            &Root::new(tmp.path()),
            day(6),
            &run("abc"),
            &[Exported::example()],
        )
        .await;

        assert!(
            matches!(
                refused,
                Err(RunError::NotByRun {
                    dataset: "exported",
                    key: Some("made_date")
                })
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn a_run_id_that_cannot_name_a_partition_is_refused() {
        assert!(RunId::new("a/b").is_err());
        assert!(RunId::new("").is_err());
        assert!(RunId::new("a=b").is_err());
    }
}
