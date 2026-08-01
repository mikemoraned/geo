//! Reading the store with SQL.
//!
//! A dataset is registered as a table by name, which handles walking its partition
//! directories and reading the geometry columns back with their CRS, so callers express
//! what they want of a dataset as a query rather than as file traversal.

use datafusion::arrow::array::RecordBatch;
use sedona::context::SedonaContext;
use sedona_geoparquet::provider::GeoParquetReadOptions;

use crate::dataset::DatasetSpec;
use crate::layer::LayerKind;
use crate::path::{Dataset, Root};

/// A failure querying the store.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("dataset {dataset} in {layer} does not exist")]
    NoSuchDataset {
        layer: &'static str,
        dataset: String,
    },
    #[error("datafusion error: {0}")]
    DataFusion(#[from] datafusion::error::DataFusionError),
    #[error("reading rows: {0}")]
    Rows(#[from] serde_arrow::Error),
}

/// The single column a counting query returns. Its name is fixed, so callers alias their
/// count to it: `SELECT COUNT(*) AS count …`.
#[derive(Debug, serde::Deserialize)]
struct Counted {
    count: i64,
}

/// Whether `dir` holds any file, at any depth below it.
///
/// A directory of empty directories is what a swept dataset leaves, and reads the same as one
/// that was never written.
fn holds_files(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry.path().is_dir() && holds_files(&entry.path()) || entry.path().is_file()
        })
    })
}

/// A SQL session over one medallion store.
pub struct Query {
    root: Root,
    ctx: SedonaContext,
}

impl Query {
    pub fn new(root: Root) -> Self {
        Self {
            root,
            ctx: SedonaContext::new(),
        }
    }

    /// Register `dataset` from `layer` under `table`, so queries can name it.
    ///
    /// A dataset that has never been written is not an error the caller has to
    /// distinguish by hand: [`QueryError::NoSuchDataset`] says so, and
    /// [`Self::register_if_present`] treats it as an empty table instead.
    pub async fn register<L: LayerKind>(
        &self,
        dataset: DatasetSpec<L>,
        table: &str,
    ) -> Result<(), QueryError> {
        self.register_at(&self.root.dataset(dataset), table).await
    }

    /// Register one partition of a dataset under `table`, for a dataset whose partitions
    /// hold different schemas and so cannot be read as a single table.
    ///
    /// A dataset holding no files is absent, whether it was never written or a rebuild has
    /// since swept every partition away: both leave a reader with nothing to read, and the
    /// directory a sweep leaves behind is not something a caller should have to know about.
    pub async fn register_at<L: LayerKind>(
        &self,
        dataset: &Dataset<L>,
        table: &str,
    ) -> Result<(), QueryError> {
        let dir = dataset.dir();
        if !holds_files(&dir) {
            return Err(QueryError::NoSuchDataset {
                layer: dataset.layer(),
                dataset: dataset.name().to_string(),
            });
        }
        let df = self
            .ctx
            .read_parquet(dir.display().to_string(), GeoParquetReadOptions::default())
            .await?;
        self.ctx.ctx.register_table(table, df.into_view())?;
        Ok(())
    }

    /// Register `dataset` if it exists, reporting whether it did. A dataset with no files
    /// yet leaves `table` unregistered, so a query naming it is a planning error rather
    /// than a silent empty result.
    pub async fn register_if_present<L: LayerKind>(
        &self,
        dataset: DatasetSpec<L>,
        table: &str,
    ) -> Result<bool, QueryError> {
        match self.register(dataset, table).await {
            Ok(()) => Ok(true),
            Err(QueryError::NoSuchDataset { .. }) => Ok(false),
            Err(err) => Err(err),
        }
    }

    /// Run `sql` and collect the result.
    pub async fn sql(&self, sql: &str) -> Result<Vec<RecordBatch>, QueryError> {
        Ok(self.ctx.sql(sql).await?.collect().await?)
    }

    /// Run a `SELECT COUNT(*) …` and return the count. The query must select exactly one
    /// row of one column.
    pub async fn count(&self, sql: &str) -> Result<i64, QueryError> {
        Ok(self
            .rows::<Counted>(sql)
            .await?
            .first()
            .map_or(0, |counted| counted.count))
    }

    /// Run `sql` and deserialise the result into `T`, for queries whose columns map onto a
    /// plain Rust type.
    pub async fn rows<T>(&self, sql: &str) -> Result<Vec<T>, QueryError>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        let mut rows = Vec::new();
        for batch in self.sql(sql).await? {
            rows.extend(serde_arrow::from_record_batch::<Vec<T>>(&batch)?);
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::array::{Int64Array, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};
    use chrono::{TimeZone, Utc};
    use serde::Deserialize;

    use super::*;
    use crate::layer::layers;

    const THING: DatasetSpec<layers::Bronze> = DatasetSpec::partitioned("thing", "kind");
    const NOTHING: DatasetSpec<layers::Silver> = DatasetSpec::partitioned("nothing", "kind");

    #[derive(Debug, Deserialize, PartialEq)]
    struct Row {
        id: i64,
        name: String,
    }

    async fn store_with_rows(dir: &std::path::Path, ids: Vec<i64>, names: Vec<&str>) -> Root {
        let root = Root::new(dir);
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("name", DataType::Utf8, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int64Array::from(ids)),
                Arc::new(StringArray::from(names)),
            ],
        )
        .unwrap();
        root.dataset(THING)
            .partition("kind", "a")
            .unwrap()
            .append(
                Utc.with_ymd_and_hms(2026, 7, 26, 9, 0, 0).unwrap(),
                &[batch],
            )
            .await
            .unwrap();
        root
    }

    #[tokio::test]
    async fn a_registered_dataset_can_be_queried_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        let root = store_with_rows(tmp.path(), vec![1, 2, 3], vec!["a", "b", "c"]).await;
        let query = Query::new(root);
        query.register(THING, "thing").await.unwrap();

        let rows: Vec<Row> = query
            .rows("SELECT id, name FROM thing WHERE id > 1 ORDER BY id")
            .await
            .unwrap();

        assert_eq!(
            rows,
            vec![
                Row {
                    id: 2,
                    name: "b".into()
                },
                Row {
                    id: 3,
                    name: "c".into()
                }
            ]
        );
    }

    /// Every partition of a dataset is one table: registering walks the partition
    /// directories so callers never do.
    #[tokio::test]
    async fn registering_covers_every_partition_of_the_dataset() {
        let tmp = tempfile::tempdir().unwrap();
        let root = store_with_rows(tmp.path(), vec![1], vec!["a"]).await;
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("name", DataType::Utf8, false),
        ]));
        root.dataset(THING)
            .partition("kind", "b")
            .unwrap()
            .append(
                Utc.with_ymd_and_hms(2026, 7, 26, 9, 0, 0).unwrap(),
                &[RecordBatch::try_new(
                    schema,
                    vec![
                        Arc::new(Int64Array::from(vec![2])),
                        Arc::new(StringArray::from(vec!["b"])),
                    ],
                )
                .unwrap()],
            )
            .await
            .unwrap();

        let query = Query::new(root);
        query.register(THING, "thing").await.unwrap();
        let rows: Vec<Row> = query
            .rows("SELECT id, name FROM thing ORDER BY id")
            .await
            .unwrap();

        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn a_count_comes_back_as_a_number() {
        let tmp = tempfile::tempdir().unwrap();
        let root = store_with_rows(tmp.path(), vec![1, 2, 3], vec!["a", "b", "c"]).await;
        let query = Query::new(root);
        query.register(THING, "thing").await.unwrap();

        assert_eq!(
            query
                .count("SELECT COUNT(*) AS count FROM thing WHERE id > 1")
                .await
                .unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn a_dataset_that_was_never_written_is_reported_as_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let query = Query::new(Root::new(tmp.path()));

        assert!(matches!(
            query.register(NOTHING, "nothing").await,
            Err(QueryError::NoSuchDataset { .. })
        ));
        assert!(!query.register_if_present(NOTHING, "nothing").await.unwrap());
    }

    /// A rebuild that produces nothing sweeps every partition and leaves the dataset's own
    /// directory standing. That is not a dataset a reader can read, so it reads as absent
    /// rather than as a schema the engine cannot infer.
    #[tokio::test]
    async fn a_dataset_swept_empty_is_reported_as_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        std::fs::create_dir_all(root.dataset(NOTHING).dir()).unwrap();
        let query = Query::new(root);

        assert!(!query.register_if_present(NOTHING, "nothing").await.unwrap());
    }
}
