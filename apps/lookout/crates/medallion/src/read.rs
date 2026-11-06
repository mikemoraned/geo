use std::marker::PhantomData;

use datafusion::dataframe::DataFrame;
use datafusion::logical_expr::{Expr, SortExpr};

use crate::query::QueryError;
use crate::rows::{Row, fields};

#[derive(Debug)]
pub struct Read<T> {
    frame: DataFrame,
    rows: PhantomData<T>,
}

impl<T: Row> Read<T> {
    pub(crate) fn new(frame: DataFrame) -> Self {
        Self {
            frame,
            rows: PhantomData,
        }
    }

    pub fn filter(self, predicate: Expr) -> Result<Self, QueryError> {
        Ok(Self::new(self.frame.filter(predicate)?))
    }

    pub fn sort(self, by: Vec<SortExpr>) -> Result<Self, QueryError> {
        Ok(Self::new(self.frame.sort(by)?))
    }

    pub fn limit(self, rows: usize) -> Result<Self, QueryError> {
        Ok(Self::new(self.frame.limit(0, Some(rows))?))
    }

    pub async fn rows(self) -> Result<Vec<T>, QueryError> {
        let mut rows = Vec::new();
        for batch in self.projected()?.collect().await? {
            rows.extend(serde_arrow::from_record_batch::<Vec<T>>(&batch)?);
        }
        Ok(rows)
    }

    fn projected(self) -> Result<DataFrame, QueryError> {
        let columns: Vec<String> = fields::<T>()?
            .iter()
            .map(|field| field.name().clone())
            .collect();
        let columns: Vec<&str> = columns.iter().map(String::as_str).collect();
        Ok(self.frame.select_columns(&columns)?)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::{DateTime, NaiveDate, TimeZone, Utc};
    use datafusion::arrow::util::pretty::pretty_format_batches;
    use datafusion::prelude::{col, lit};
    use serde::{Deserialize, Serialize};

    use domain::places::BERLIN;

    use super::*;
    use crate::country::Country;
    use crate::dataset::DatasetSpec;
    use crate::derive::{GeographyRow, write_geography_rows};
    use crate::layer::layers;
    use crate::path::Root;
    use crate::query::Query;
    use crate::rows::{Dated, Geometry};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Reading {
        reading_id: String,
        #[serde(with = "chrono::serde::ts_milliseconds")]
        taken_at: DateTime<Utc>,
        level: f64,
    }

    impl Row for Reading {
        type Layer = layers::Silver;
        const DATASET: DatasetSpec<Self::Layer> = DatasetSpec::partitioned("reading", "taken_date");
        const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
        const INSTANTS: &'static [&'static str] = &["taken_at"];

        fn example() -> Self {
            Reading {
                reading_id: "example".to_string(),
                taken_at: DateTime::UNIX_EPOCH,
                level: 0.0,
            }
        }
    }

    impl Dated for Reading {
        fn partition_date(&self) -> NaiveDate {
            self.taken_at.date_naive()
        }
    }

    fn reading(n: u32) -> Reading {
        Reading {
            reading_id: format!("reading-{n}"),
            taken_at: Utc
                .with_ymd_and_hms(2026, 7, 20 + n % 3, 9, 0, 0)
                .single()
                .expect("an instant"),
            level: f64::from(n),
        }
    }

    fn readings(count: u32) -> Vec<Reading> {
        (1..=count).map(reading).collect()
    }

    async fn stored(dir: &Path, readings: &[Reading]) -> Root {
        let root = Root::new(dir);
        let rows: Vec<GeographyRow<Reading, geo_types::Point<f64>>> = readings
            .iter()
            .cloned()
            .map(|row| GeographyRow {
                row,
                geography: BERLIN,
                country: Country::Germany,
            })
            .collect();
        write_geography_rows(&root, &rows)
            .await
            .expect("write the readings");
        root
    }

    async fn read(root: Root) -> Read<Reading> {
        Query::new(root)
            .read_of_country::<Reading>(Country::Germany)
            .await
            .expect("a read of the readings")
    }

    fn ids(readings: &[Reading]) -> Vec<String> {
        readings
            .iter()
            .map(|reading| reading.reading_id.clone())
            .collect()
    }

    #[tokio::test]
    async fn a_read_returns_every_row_of_the_country_as_it_was_written() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path(), &readings(6)).await;

        let mut read = read(root).await.rows().await.expect("the readings");
        read.sort_by(|one, other| one.reading_id.cmp(&other.reading_id));

        assert_eq!(read, readings(6));
    }

    #[tokio::test]
    async fn a_filter_keeps_only_the_rows_it_holds_for() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path(), &readings(6)).await;

        let mut kept = read(root)
            .await
            .filter(col("level").gt_eq(lit(4.0)))
            .expect("a filter")
            .rows()
            .await
            .expect("the readings");
        kept.sort_by(|one, other| one.reading_id.cmp(&other.reading_id));

        assert_eq!(ids(&kept), ["reading-4", "reading-5", "reading-6"]);
    }

    #[tokio::test]
    async fn a_sort_and_a_limit_keep_the_leading_rows_in_order() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path(), &readings(6)).await;

        let leading = read(root)
            .await
            .sort(vec![col("level").sort(false, false)])
            .expect("a sort")
            .limit(2)
            .expect("a limit")
            .rows()
            .await
            .expect("the readings");

        assert_eq!(ids(&leading), ["reading-6", "reading-5"]);
    }

    async fn scan_of(read: Read<Reading>, analyze: Analyze) -> String {
        let explained = read
            .projected()
            .expect("a projection")
            .explain(false, analyze == Analyze::Yes)
            .expect("a plan")
            .collect()
            .await
            .expect("the plan");
        let plan = pretty_format_batches(&explained)
            .expect("a printable plan")
            .to_string();
        plan.lines()
            .find(|line| line.contains("DataSourceExec"))
            .unwrap_or_else(|| panic!("no scan in\n{plan}"))
            .to_string()
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Analyze {
        Yes,
        No,
    }

    #[tokio::test]
    async fn the_scan_reads_the_columns_of_the_row_type_and_no_others() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path(), &readings(6)).await;

        let scan = scan_of(read(root).await, Analyze::No).await;

        assert!(
            scan.contains("projection=[reading_id, taken_at, level]"),
            "{scan}"
        );
    }

    #[tokio::test]
    async fn a_filter_skips_each_file_whose_statistics_rule_it_out() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path(), &readings(6)).await;
        let read = read(root)
            .await
            .filter(col("level").gt_eq(lit(5.0)))
            .expect("a filter");

        let scan = scan_of(read, Analyze::Yes).await;

        assert!(
            scan.contains("files_ranges_pruned_statistics=3 total \u{2192} 2 matched"),
            "{scan}"
        );
    }

    #[tokio::test]
    async fn a_read_of_a_country_the_dataset_does_not_hold_is_refused() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path(), &readings(1)).await;

        let refused = Query::new(root)
            .read_of_country::<Reading>(Country::UnitedKingdom)
            .await;

        assert!(
            matches!(refused, Err(QueryError::NoSuchDataset { .. })),
            "{refused:?}"
        );
    }
}
