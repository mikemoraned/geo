//! Reading the store with SQL.
//!
//! A dataset is registered as a table by name, which handles walking its partition
//! directories and reading the geometry columns back with their CRS, so callers express
//! what they want of a dataset as a query rather than as file traversal.

use std::collections::HashMap;

use datafusion::arrow::array::RecordBatch;
use datafusion::common::ParamValues;
use datafusion::scalar::ScalarValue;
use sedona::context::SedonaContext;
use sedona_geoparquet::provider::GeoParquetReadOptions;

use crate::dataset::DatasetSpec;
use crate::layer::LayerKind;
use crate::path::{Dataset, Root};
use crate::table::SilverTarget;

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

    /// Register the silver dataset `target` names, under that name.
    pub async fn register_silver(&self, target: &SilverTarget) -> Result<(), QueryError> {
        self.register_by_name(target.spec()).await
    }

    /// Register `dataset` as a table of its own name, for a query that reads it as what it
    /// is rather than under a name chosen for the query.
    pub async fn register_by_name<L: LayerKind>(
        &self,
        dataset: DatasetSpec<L>,
    ) -> Result<(), QueryError> {
        self.register(dataset, dataset.name).await
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
        if !dataset.holds_files() {
            return Err(QueryError::NoSuchDataset {
                layer: dataset.layer(),
                dataset: dataset.name().to_string(),
            });
        }
        let dir = dataset.dir();
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

    /// Run `sql` with `params` bound to its `$name` placeholders, and collect the result.
    ///
    /// A parameter is bound as a value, so an id carrying a quote reads as an id that does
    /// not exist rather than as more query.
    ///
    /// Always at least one batch: a query matching nothing answers with an empty one under
    /// the columns it would have returned, so the result describes itself either way.
    pub async fn sql_with_params(
        &self,
        sql: &str,
        params: HashMap<String, ScalarValue>,
    ) -> Result<Vec<RecordBatch>, QueryError> {
        let df = self
            .ctx
            .sql(sql)
            .await?
            .with_param_values(ParamValues::from(params))?;
        let schema = df.schema().inner().clone();
        let batches = df.collect().await?;

        Ok(match batches.is_empty() {
            true => vec![RecordBatch::new_empty(schema)],
            false => batches,
        })
    }

    /// [`Self::sql_with_params`] with no parameters bound.
    pub async fn sql(&self, sql: &str) -> Result<Vec<RecordBatch>, QueryError> {
        self.sql_with_params(sql, HashMap::new()).await
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
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::layer::layers;
    use crate::rows::Row as _;

    const THING: DatasetSpec<layers::Bronze> = DatasetSpec::partitioned("thing", "kind");
    const NOTHING: DatasetSpec<layers::Silver> = DatasetSpec::partitioned("nothing", "kind");

    #[derive(Debug, Deserialize, PartialEq)]
    struct Row {
        id: i64,
        name: String,
    }

    /// A silver dataset with a row type behind it, to register by name.
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct PassRow {
        id: i64,
        name: String,
    }

    impl crate::rows::Row for PassRow {
        type Layer = layers::Silver;
        const DATASET: DatasetSpec<Self::Layer> = DatasetSpec::partitioned("pass", "kind");
    }

    async fn store_with_rows(dir: &std::path::Path, ids: Vec<i64>, names: Vec<&str>) -> Root {
        store_dataset(dir, THING, ids, names).await
    }

    async fn store_dataset<L: LayerKind>(
        dir: &std::path::Path,
        spec: DatasetSpec<L>,
        ids: Vec<i64>,
        names: Vec<&str>,
    ) -> Root {
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
        root.dataset(spec)
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

    /// A query reading a dataset under its own name takes that name from the dataset, so
    /// the two cannot drift apart.
    #[tokio::test]
    async fn a_dataset_can_be_registered_as_a_table_of_its_own_name() {
        let tmp = tempfile::tempdir().unwrap();
        let root = store_with_rows(tmp.path(), vec![1], vec!["a"]).await;
        let query = Query::new(root);
        query.register_by_name(THING).await.unwrap();

        assert_eq!(
            query
                .count(&format!("SELECT COUNT(*) AS count FROM {}", THING.name))
                .await
                .unwrap(),
            1
        );
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

    /// A dataset a caller names rather than types is read under the name it is stored as, so
    /// the query and the store cannot drift apart.
    #[tokio::test]
    async fn a_silver_dataset_registers_under_its_own_name() {
        let tmp = tempfile::tempdir().unwrap();
        let root = store_dataset(tmp.path(), PassRow::DATASET, vec![1], vec!["a"]).await;
        let query = Query::new(root);

        query
            .register_silver(&SilverTarget::of::<PassRow>().unwrap())
            .await
            .unwrap();

        assert_eq!(
            query
                .count("SELECT COUNT(*) AS count FROM pass")
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn a_parameter_is_bound_as_a_value() {
        let tmp = tempfile::tempdir().unwrap();
        let root = store_with_rows(tmp.path(), vec![1, 2], vec!["a", "b"]).await;
        let query = Query::new(root);
        query.register(THING, "thing").await.unwrap();

        let batches = query
            .sql_with_params(
                "SELECT id, name FROM thing WHERE name = $name",
                HashMap::from([("name".to_string(), ScalarValue::Utf8(Some("b".into())))]),
            )
            .await
            .unwrap();

        let rows: Vec<Row> = serde_arrow::from_record_batch(&batches[0]).unwrap();
        assert_eq!(
            rows,
            vec![Row {
                id: 2,
                name: "b".into()
            }]
        );
    }

    /// A value is a value rather than more query, so a name that quotes its way out of the
    /// literal matches nothing instead of selecting everything.
    #[tokio::test]
    async fn a_parameter_carrying_a_quote_matches_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = store_with_rows(tmp.path(), vec![1, 2], vec!["a", "b"]).await;
        let query = Query::new(root);
        query.register(THING, "thing").await.unwrap();

        let batches = query
            .sql_with_params(
                "SELECT id FROM thing WHERE name = $name",
                HashMap::from([(
                    "name".to_string(),
                    ScalarValue::Utf8(Some("a' OR '1' = '1".into())),
                )]),
            )
            .await
            .unwrap();

        let rows: usize = batches.iter().map(RecordBatch::num_rows).sum();
        assert_eq!(rows, 0);
    }

    /// A caller handing an empty result to another engine still has the columns to hand it
    /// under, which is what the empty batch carries.
    #[tokio::test]
    async fn a_query_matching_nothing_still_describes_its_columns() {
        let tmp = tempfile::tempdir().unwrap();
        let root = store_with_rows(tmp.path(), vec![1], vec!["a"]).await;
        let query = Query::new(root);
        query.register(THING, "thing").await.unwrap();

        let batches = query
            .sql("SELECT id, name FROM thing WHERE id < 0")
            .await
            .unwrap();

        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].num_rows(), 0);
        let columns: Vec<&str> = batches[0]
            .schema_ref()
            .fields()
            .iter()
            .map(|field| field.name().as_str())
            .collect();
        assert_eq!(columns, vec!["id", "name"]);
    }
}
