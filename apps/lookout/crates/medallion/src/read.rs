use std::marker::PhantomData;

use datafusion::dataframe::DataFrame;
use datafusion::logical_expr::{Expr, SortExpr};
use serde::Deserialize;

use crate::query::QueryError;
use crate::rows::{Projection, Row, fields, from_batches, projected_columns};

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
        rows_of(self.projected()?).await
    }

    pub async fn rows_as<P: Projection<Of = T>>(self) -> Result<Vec<P>, QueryError> {
        rows_of(self.projected_as::<P>()?).await
    }

    fn projected(self) -> Result<DataFrame, QueryError> {
        let columns: Vec<String> = fields::<T>()?
            .iter()
            .map(|field| field.name().clone())
            .collect();
        self.selected(&columns)
    }

    fn projected_as<P: Projection<Of = T>>(self) -> Result<DataFrame, QueryError> {
        self.selected(&projected_columns::<P>()?)
    }

    fn selected(self, columns: &[String]) -> Result<DataFrame, QueryError> {
        let columns: Vec<&str> = columns.iter().map(String::as_str).collect();
        Ok(self.frame.select_columns(&columns)?)
    }
}

async fn rows_of<R: for<'de> Deserialize<'de>>(frame: DataFrame) -> Result<Vec<R>, QueryError> {
    Ok(from_batches(&frame.collect().await?)?)
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
    use crate::derive::{GeographyRow, write_geography_rows, write_rows};
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

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct ReadingLevel {
        reading_id: String,
        level: f64,
    }

    impl Projection for ReadingLevel {
        type Of = Reading;

        fn example() -> Self {
            ReadingLevel {
                reading_id: "example".to_string(),
                level: 0.0,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct DateOnlyRow {
        row_id: String,
        #[serde(with = "chrono::serde::ts_milliseconds")]
        counted_at: DateTime<Utc>,
        count: u32,
    }

    impl Row for DateOnlyRow {
        type Layer = layers::Silver;
        const DATASET: DatasetSpec<Self::Layer> =
            DatasetSpec::partitioned("date_only", "counted_date");
        const INSTANTS: &'static [&'static str] = &["counted_at"];

        fn example() -> Self {
            DateOnlyRow {
                row_id: "example".to_string(),
                counted_at: DateTime::UNIX_EPOCH,
                count: 0,
            }
        }
    }

    impl Dated for DateOnlyRow {
        fn partition_date(&self) -> NaiveDate {
            self.counted_at.date_naive()
        }
    }

    fn date_only_rows(count: u32) -> Vec<DateOnlyRow> {
        (1..=count)
            .map(|n| DateOnlyRow {
                row_id: format!("row-{n}"),
                counted_at: Utc
                    .with_ymd_and_hms(2026, 7, 20 + n % 3, 9, 0, 0)
                    .single()
                    .expect("an instant"),
                count: n,
            })
            .collect()
    }

    async fn written(dir: &Path, rows: &[DateOnlyRow]) -> Root {
        let root = Root::new(dir);
        write_rows(&root, rows).await.expect("write the rows");
        root
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
        scan_of_frame(read.projected().expect("a projection"), analyze).await
    }

    async fn scan_of_frame(frame: DataFrame, analyze: Analyze) -> String {
        let explained = frame
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
    async fn a_projection_returns_its_columns_of_every_row() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path(), &readings(3)).await;

        let mut levels = read(root)
            .await
            .rows_as::<ReadingLevel>()
            .await
            .expect("the levels");
        levels.sort_by(|one, other| one.reading_id.cmp(&other.reading_id));

        assert_eq!(
            levels,
            readings(3)
                .into_iter()
                .map(|reading| ReadingLevel {
                    reading_id: reading.reading_id,
                    level: reading.level,
                })
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn the_scan_of_a_projection_reads_its_columns_and_no_others() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path(), &readings(6)).await;
        let frame = read(root)
            .await
            .projected_as::<ReadingLevel>()
            .expect("a projection");

        let scan = scan_of_frame(frame, Analyze::No).await;

        assert!(scan.contains("projection=[reading_id, level]"), "{scan}");
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

    #[tokio::test]
    async fn a_read_of_a_dataset_laid_out_by_date_returns_every_row_of_every_date() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = written(tmp.path(), &date_only_rows(6)).await;

        let mut read = Query::new(root)
            .read::<DateOnlyRow>()
            .await
            .expect("a read of the date_only_rows")
            .rows()
            .await
            .expect("the date_only_rows");
        read.sort_by(|one, other| one.row_id.cmp(&other.row_id));

        assert_eq!(read, date_only_rows(6));
    }

    #[tokio::test]
    async fn a_read_of_a_dataset_laid_out_by_date_filters_on_the_date_it_is_laid_out_by() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = written(tmp.path(), &date_only_rows(6)).await;

        let mut kept = Query::new(root)
            .read::<DateOnlyRow>()
            .await
            .expect("a read of the date_only_rows")
            .filter(col("counted_date").eq(lit("2026-07-21")))
            .expect("a filter")
            .rows()
            .await
            .expect("the date_only_rows");
        kept.sort_by_key(|row| row.count);

        assert_eq!(kept.iter().map(|row| row.count).collect::<Vec<_>>(), [1, 4]);
    }

    #[tokio::test]
    async fn a_read_of_a_dataset_laid_out_by_date_filters_in_the_engine() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = written(tmp.path(), &date_only_rows(6)).await;

        let kept = Query::new(root)
            .read::<DateOnlyRow>()
            .await
            .expect("a read of the date_only_rows")
            .filter(col("row_id").in_list(vec![lit("row-2"), lit("row-5")], false))
            .expect("a filter")
            .sort(vec![col("row_id").sort(true, false)])
            .expect("a sort")
            .rows()
            .await
            .expect("the date_only_rows");

        assert_eq!(kept.iter().map(|row| row.count).collect::<Vec<_>>(), [2, 5]);
    }

    #[tokio::test]
    async fn a_read_of_every_country_at_once_is_refused() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = stored(tmp.path(), &readings(1)).await;

        let refused = Query::new(root).read::<Reading>().await;

        assert!(
            matches!(
                refused,
                Err(QueryError::CountryNeeded { dataset: "reading" })
            ),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn a_read_of_one_country_of_a_dataset_with_no_country_level_is_refused() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = written(tmp.path(), &date_only_rows(1)).await;

        let refused = Query::new(root)
            .read_of_country::<DateOnlyRow>(Country::Germany)
            .await;

        assert!(
            matches!(
                refused,
                Err(QueryError::NoCountryLevel {
                    dataset: "date_only"
                })
            ),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn a_read_of_a_dataset_never_written_is_refused() {
        let tmp = tempfile::tempdir().expect("a temporary directory");

        let refused = Query::new(Root::new(tmp.path()))
            .read::<DateOnlyRow>()
            .await;

        assert!(
            matches!(refused, Err(QueryError::NoSuchDataset { .. })),
            "{refused:?}"
        );
    }
}
