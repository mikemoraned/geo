use std::collections::{BTreeSet, HashMap};

use datafusion::arrow::array::RecordBatch;
use datafusion::common::ParamValues;
use datafusion::scalar::ScalarValue;
use datafusion::sql::TableReference;
use datafusion::sql::parser::DFParser;
use datafusion::sql::resolve::resolve_table_references;
use sedona::context::SedonaContext;
use sedona_geoparquet::provider::GeoParquetReadOptions;

use crate::dataset::DatasetSpec;
use crate::layer::LayerKind;
use crate::path::{Dataset, Root};
use crate::table::SilverTarget;

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

pub fn table_references(sql: &str) -> Result<Vec<String>, QueryError> {
    let mut names = BTreeSet::new();

    for statement in DFParser::parse_sql(sql)? {
        let (referenced, _ctes) = resolve_table_references(&statement, true)?;
        names.extend(referenced.iter().map(TableReference::to_string));
    }

    Ok(names.into_iter().collect())
}

#[derive(Debug, serde::Deserialize)]
struct Counted {
    count: i64,
}

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

    pub async fn register<L: LayerKind>(
        &self,
        dataset: DatasetSpec<L>,
        table: &str,
    ) -> Result<(), QueryError> {
        self.register_at(&self.root.dataset(dataset), table).await
    }

    pub async fn register_silver(&self, target: &SilverTarget) -> Result<(), QueryError> {
        self.register_by_name(target.spec()).await
    }

    pub async fn register_by_name<L: LayerKind>(
        &self,
        dataset: DatasetSpec<L>,
    ) -> Result<(), QueryError> {
        self.register(dataset, dataset.name).await
    }

    pub async fn register_at<L: LayerKind>(
        &self,
        dataset: &Dataset<L>,
        table: &str,
    ) -> Result<(), QueryError> {
        if !dataset.is_filled() {
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

    pub async fn sql(&self, sql: &str) -> Result<Vec<RecordBatch>, QueryError> {
        self.sql_with_params(sql, HashMap::new()).await
    }

    pub async fn count(&self, sql: &str) -> Result<i64, QueryError> {
        Ok(self
            .rows::<Counted>(sql)
            .await?
            .first()
            .map_or(0, |counted| counted.count))
    }

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

    #[tokio::test]
    async fn a_dataset_swept_empty_is_reported_as_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let root = Root::new(tmp.path());
        std::fs::create_dir_all(root.dataset(NOTHING).dir()).unwrap();
        let query = Query::new(root);

        assert!(!query.register_if_present(NOTHING, "nothing").await.unwrap());
    }

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

    #[test]
    fn the_tables_a_query_reads_are_the_ones_it_names() {
        assert_eq!(
            table_references("SELECT id FROM thing WHERE id > 1").unwrap(),
            vec!["thing"]
        );
    }

    #[test]
    fn a_table_joined_to_another_is_named_once_each() {
        let names =
            table_references("SELECT * FROM thing t, other o WHERE t.id = o.id AND t.id > 1")
                .unwrap();

        assert_eq!(names, vec!["other", "thing"]);
    }

    #[test]
    fn a_table_read_twice_is_named_once() {
        let names =
            table_references("SELECT id FROM thing UNION ALL SELECT id FROM thing WHERE id > 1")
                .unwrap();

        assert_eq!(names, vec!["thing"]);
    }

    #[test]
    fn a_cte_is_not_a_table_to_look_for() {
        let names = table_references(
            "WITH recent AS (SELECT * FROM thing WHERE id > 1) SELECT * FROM recent",
        )
        .unwrap();

        assert_eq!(names, vec!["thing"]);
    }

    #[test]
    fn a_name_inside_a_literal_is_not_a_table() {
        let names = table_references("SELECT id FROM thing WHERE name = 'from other'").unwrap();

        assert_eq!(names, vec!["thing"]);
    }

    #[test]
    fn a_query_reading_no_table_names_none() {
        assert!(table_references("SELECT 1").unwrap().is_empty());
    }

    #[test]
    fn sql_that_does_not_parse_is_an_error_rather_than_no_tables() {
        assert!(table_references("SELECT * FROM (").is_err());
    }

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
