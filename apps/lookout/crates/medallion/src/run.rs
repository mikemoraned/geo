use std::path::{Path, PathBuf};

use arrow::array::RecordBatch;
use chrono::NaiveDate;
use nutype::nutype;

use crate::dataset::DatasetSpec;
use crate::layer::layers;
use crate::partition::{Partition, PartitionValue, PathError};
use crate::path::{Replaced, Root};
use crate::rows::{Geometry, Row, RowError, batch};
use crate::table::{GoldTarget, TableError, TableWritten};
use crate::write::WriteError;

pub const RUN_DATE: &str = "run_date";
pub const RUN_ID: &str = "run_id";

const BY_RUN: [&str; 2] = [RUN_DATE, RUN_ID];

#[nutype(
    validate(with = partition_value, error = PathError),
    derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Display)
)]
pub struct RunId(String);

fn partition_value(id: &str) -> Result<(), PathError> {
    PartitionValue::new(id).map(|_| ())
}

impl RunId {
    pub fn new(id: impl Into<String>) -> Result<Self, PathError> {
        Self::try_new(id)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error(
        "{dataset} is not laid out by run: its partition levels are {levels:?}, not {BY_RUN:?}"
    )]
    NotByRun {
        dataset: &'static str,
        levels: Vec<&'static str>,
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
    Table(#[from] TableError),
    #[error(transparent)]
    Write(#[from] WriteError),
}

pub async fn write_run<T: Row<Layer = layers::Gold>>(
    root: &Root,
    date: NaiveDate,
    run_id: &RunId,
    rows: &[T],
) -> Result<PathBuf, RunError> {
    if T::GEOMETRY != Geometry::Absent {
        return Err(RunError::Geometry {
            dataset: T::DATASET.name,
        });
    }
    write_run_batch(root, T::DATASET, date, run_id, &batch(rows)?).await
}

pub async fn write_run_table(
    root: &Root,
    target: &GoldTarget,
    date: NaiveDate,
    run_id: &RunId,
    table: &[RecordBatch],
) -> Result<TableWritten, RunError> {
    if target.geometry != Geometry::Absent {
        return Err(RunError::Geometry {
            dataset: target.name(),
        });
    }
    let rows = target.rows(table)?;
    write_run_batch(root, target.spec(), date, run_id, &rows).await?;
    Ok(TableWritten {
        rows: rows.num_rows(),
        partitions: Replaced {
            written: 1,
            removed: 0,
        },
    })
}

pub(crate) async fn write_run_batch(
    root: &Root,
    dataset: DatasetSpec<layers::Gold>,
    date: NaiveDate,
    run_id: &RunId,
    rows: &RecordBatch,
) -> Result<PathBuf, RunError> {
    let levels = dataset.partition_levels();
    if levels != BY_RUN {
        return Err(RunError::NotByRun {
            dataset: dataset.name,
            levels,
        });
    }
    let runs = root.dataset(dataset);
    if let Some(written_on) = written_on(&runs.dir(), run_id) {
        return Err(RunError::Written {
            dataset: dataset.name,
            run_id: run_id.clone(),
            written_on,
        });
    }
    let partition = runs.on_date(date)?.partition(RUN_ID, run_id)?;
    Ok(partition.replace_with(std::slice::from_ref(rows)).await?)
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

    use std::sync::Arc;

    use arrow::array::{ArrayRef, StringArray};
    use datafusion::prelude::{col, lit};
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::query::{Query, QueryError};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Scored {
        notification_id: String,
        score: f64,
    }

    impl Row for Scored {
        type Layer = layers::Gold;
        const DATASET: DatasetSpec<Self::Layer> =
            DatasetSpec::partitioned_within("scored", RUN_DATE, &[RUN_ID]);

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

    fn target() -> GoldTarget {
        GoldTarget::of::<Scored>().expect("a target")
    }

    fn scored_table(count: u32) -> RecordBatch {
        batch(&scored(count)).expect("a table of scores")
    }

    fn with_column(table: &RecordBatch, name: &str, column: ArrayRef) -> RecordBatch {
        let mut columns: Vec<(String, ArrayRef)> = table
            .schema()
            .fields()
            .iter()
            .zip(table.columns())
            .map(|(field, column)| (field.name().clone(), Arc::clone(column)))
            .collect();
        columns.push((name.to_string(), column));
        RecordBatch::try_from_iter(columns).expect("a table with the column")
    }

    fn without_column(table: &RecordBatch, name: &str) -> RecordBatch {
        let index = table.schema().index_of(name).expect("the column");
        let kept: Vec<usize> = (0..table.num_columns()).filter(|&n| n != index).collect();
        table.project(&kept).expect("a table without the column")
    }

    #[tokio::test]
    async fn a_table_written_as_a_run_reports_its_rows_and_one_partition() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = Root::new(tmp.path());

        let written = write_run_table(&root, &target(), day(6), &run("abc"), &[scored_table(3)])
            .await
            .expect("a written run");

        assert_eq!(
            written,
            TableWritten {
                rows: 3,
                partitions: Replaced {
                    written: 1,
                    removed: 0
                },
            }
        );
        assert_eq!(read_back(tmp.path()).await, scored(3));
    }

    #[tokio::test]
    async fn a_tables_columns_are_written_in_the_row_types_order() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = Root::new(tmp.path());
        let table = scored_table(3);
        let reversed: Vec<usize> = (0..table.num_columns()).rev().collect();

        write_run_table(
            &root,
            &target(),
            day(6),
            &run("abc"),
            &[table.project(&reversed).expect("the columns reversed")],
        )
        .await
        .expect("a written run");

        assert_eq!(read_back(tmp.path()).await, scored(3));
    }

    #[tokio::test]
    async fn a_table_lacking_a_column_of_the_row_type_is_refused() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let lacking = without_column(&scored_table(3), "score");

        let refused = write_run_table(
            &Root::new(tmp.path()),
            &target(),
            day(6),
            &run("abc"),
            &[lacking],
        )
        .await;

        assert!(
            matches!(
                &refused,
                Err(RunError::Table(TableError::Missing { column, .. })) if column == "score"
            ),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn a_table_holding_a_column_the_row_type_does_not_define_is_refused() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let extra = with_column(
            &scored_table(3),
            RUN_ID,
            Arc::new(StringArray::from(vec!["abc"; 3])),
        );

        let refused = write_run_table(
            &Root::new(tmp.path()),
            &target(),
            day(6),
            &run("abc"),
            &[extra],
        )
        .await;

        assert!(
            matches!(
                &refused,
                Err(RunError::Table(TableError::Unexpected { columns, .. })) if *columns == [RUN_ID]
            ),
            "{refused:?}"
        );
        assert!(!tmp.path().join("gold").exists(), "nothing written");
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
    async fn a_read_filtered_on_one_run_returns_that_runs_rows_alone() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = Root::new(tmp.path());
        write_run(&root, day(6), &run("abc"), &scored(3))
            .await
            .expect("a written run");
        write_run(&root, day(7), &run("def"), &scored(2))
            .await
            .expect("another written run");

        let mut rows = Query::new(root)
            .read::<Scored>()
            .await
            .expect("a read of the runs")
            .filter(col(RUN_ID).eq(lit("abc")))
            .expect("a filter")
            .rows()
            .await
            .expect("the rows");
        rows.sort_by(|one, other| one.notification_id.cmp(&other.notification_id));

        assert_eq!(rows, scored(3));
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
                &refused,
                Err(RunError::NotByRun {
                    dataset: "exported",
                    levels,
                }) if *levels == ["made_date"]
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

    #[derive(Debug, PartialEq, Deserialize)]
    struct RunCount {
        run_date: String,
        run_id: String,
        rows: i64,
    }

    async fn two_runs(dir: &Path) -> Root {
        let root = Root::new(dir);
        write_run(&root, day(6), &run("abc"), &scored(2))
            .await
            .expect("a written run");
        write_run(&root, day(7), &run("def"), &scored(1))
            .await
            .expect("a written run");
        root
    }

    #[tokio::test]
    async fn a_query_of_a_gold_dataset_sees_each_rows_run() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let query = Query::new(two_runs(tmp.path()).await);
        query
            .register_gold(Scored::DATASET)
            .await
            .expect("a registered dataset");

        let counts: Vec<RunCount> = query
            .rows(
                "SELECT run_date, run_id, COUNT(*) AS rows FROM scored \
                 GROUP BY run_date, run_id ORDER BY run_id",
            )
            .await
            .expect("the counts");

        assert_eq!(
            counts,
            [
                RunCount {
                    run_date: "2026-11-06".to_string(),
                    run_id: "abc".to_string(),
                    rows: 2
                },
                RunCount {
                    run_date: "2026-11-07".to_string(),
                    run_id: "def".to_string(),
                    rows: 1
                },
            ]
        );
    }

    #[tokio::test]
    async fn a_query_picks_one_run_by_its_id() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let query = Query::new(two_runs(tmp.path()).await);
        query
            .register_gold(Scored::DATASET)
            .await
            .expect("a registered dataset");

        let picked: Vec<Scored> = query
            .rows("SELECT notification_id, score FROM scored WHERE run_id = 'def'")
            .await
            .expect("the run's rows");

        assert_eq!(picked, scored(1));
    }

    #[tokio::test]
    async fn a_gold_dataset_never_written_is_refused() {
        let tmp = tempfile::tempdir().expect("a temporary directory");

        let refused = Query::new(Root::new(tmp.path()))
            .register_gold(Scored::DATASET)
            .await;

        assert!(
            matches!(refused, Err(QueryError::NoSuchDataset { .. })),
            "{refused:?}"
        );
    }
}
