//! Reading the store with SQL.
//!
//! A dataset is registered as a table by name, which handles walking its partition
//! directories and reading the geometry columns back with their CRS, so callers express
//! what they want of a dataset as a query rather than as file traversal.

use datafusion::arrow::array::RecordBatch;
use sedona::context::SedonaContext;
use sedona_geoparquet::provider::GeoParquetReadOptions;

use crate::layer::Layer;
use crate::path::Root;

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
    pub async fn register(
        &self,
        layer: Layer,
        dataset: &str,
        table: &str,
    ) -> Result<(), QueryError> {
        let dir = self.root.dataset(layer, dataset).dir();
        if !dir.exists() {
            return Err(QueryError::NoSuchDataset {
                layer: layer.as_str(),
                dataset: dataset.to_string(),
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
    pub async fn register_if_present(
        &self,
        layer: Layer,
        dataset: &str,
        table: &str,
    ) -> Result<bool, QueryError> {
        match self.register(layer, dataset, table).await {
            Ok(()) => Ok(true),
            Err(QueryError::NoSuchDataset { .. }) => Ok(false),
            Err(err) => Err(err),
        }
    }

    /// Run `sql` and collect the result.
    pub async fn sql(&self, sql: &str) -> Result<Vec<RecordBatch>, QueryError> {
        Ok(self.ctx.sql(sql).await?.collect().await?)
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
    use serde::Deserialize;

    use super::*;

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
        root.dataset(Layer::Bronze, "thing")
            .partition("kind", "a")
            .unwrap()
            .rebuild(&[batch])
            .await
            .unwrap();
        root
    }

    #[tokio::test]
    async fn a_registered_dataset_can_be_queried_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        let root = store_with_rows(tmp.path(), vec![1, 2, 3], vec!["a", "b", "c"]).await;
        let query = Query::new(root);
        query
            .register(Layer::Bronze, "thing", "thing")
            .await
            .unwrap();

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
        root.dataset(Layer::Bronze, "thing")
            .partition("kind", "b")
            .unwrap()
            .rebuild(&[RecordBatch::try_new(
                schema,
                vec![
                    Arc::new(Int64Array::from(vec![2])),
                    Arc::new(StringArray::from(vec!["b"])),
                ],
            )
            .unwrap()])
            .await
            .unwrap();

        let query = Query::new(root);
        query
            .register(Layer::Bronze, "thing", "thing")
            .await
            .unwrap();
        let rows: Vec<Row> = query
            .rows("SELECT id, name FROM thing ORDER BY id")
            .await
            .unwrap();

        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn a_dataset_that_was_never_written_is_reported_as_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let query = Query::new(Root::new(tmp.path()));

        assert!(matches!(
            query.register(Layer::Silver, "nothing", "nothing").await,
            Err(QueryError::NoSuchDataset { .. })
        ));
        assert!(!query
            .register_if_present(Layer::Silver, "nothing", "nothing")
            .await
            .unwrap());
    }
}
