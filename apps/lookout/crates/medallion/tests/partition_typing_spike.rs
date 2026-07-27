//! Spike: what does a predicate over a date-valued Hive partition key actually do in
//! SedonaDB, when the reader lets the partition columns be inferred?
//!
//! Asking because inference types every key `Utf8View` (`sedona-geoparquet`'s provider maps
//! inferred names to that type unconditionally), so a `WHERE key >= DATE '…'` compares a
//! string to a date. Three things need separating:
//!
//!   1. what type the key comes back as, and whether a date-typed predicate is accepted,
//!      rejected, or silently coerced;
//!   2. whether such a predicate *prunes*, i.e. whether the files of excluded partitions are
//!      opened at all;
//!   3. whether declaring the type (`with_table_partition_cols`) changes either answer.
//!
//! **On evidence for (2):** corrupting a file the predicate excludes — which is how this was
//! answered for DuckDB — does not work here. A registered table serves content cached at
//! registration: after corrupting a partition, a query selecting only that partition still
//! returns its row, while a *fresh* context cannot open the store at all. So the physical
//! plan's `file_groups` is the evidence, and it is read below rather than a query's success.
//!
//! Delete once the answer is recorded.

use std::sync::Arc;

use arrow::array::{Int64Array, RecordBatch};
use arrow::datatypes::{DataType, Field, Schema};
use medallion::{DatasetSpec, Layer, Root};
use sedona::context::SedonaContext;
use sedona_geoparquet::provider::GeoParquetReadOptions;

const READING: DatasetSpec = DatasetSpec::partitioned(Layer::Bronze, "reading", "ingested_date");

/// Three partitions, one row each, so a predicate's reach shows in both rows and file count.
const DATES: [&str; 3] = ["2026-07-20", "2026-07-24", "2026-07-28"];

/// Every predicate below excludes exactly the first of [`DATES`], so a plan naming two files
/// has pruned and one naming three has not.
const PREDICATES: [&str; 3] = [
    "ingested_date >= DATE '2026-07-24'",
    "ingested_date >= '2026-07-24'",
    "ingested_date::DATE >= DATE '2026-07-24'",
];

async fn store(dir: &std::path::Path) -> Root {
    let root = Root::new(dir);
    let schema = Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)]));
    for (i, date) in DATES.iter().enumerate() {
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(Int64Array::from(vec![i as i64]))],
        )
        .unwrap();
        root.dataset(READING)
            .partition("ingested_date", date)
            .unwrap()
            .rebuild(&[batch])
            .await
            .unwrap();
    }
    root
}

async fn context(root: &Root, options: GeoParquetReadOptions<'_>) -> SedonaContext {
    let ctx = SedonaContext::new();
    let dir = root.dataset(READING).dir();
    let df = ctx
        .read_parquet(dir.display().to_string(), options)
        .await
        .unwrap();
    ctx.ctx.register_table("reading", df.into_view()).unwrap();
    ctx
}

async fn run(ctx: &SedonaContext, sql: &str) -> Result<Vec<RecordBatch>, String> {
    match ctx.sql(sql).await {
        Ok(df) => df.collect().await.map_err(|e| e.to_string()),
        Err(err) => Err(err.to_string()),
    }
}

fn rows(batches: &[RecordBatch]) -> usize {
    batches.iter().map(RecordBatch::num_rows).sum()
}

/// The one value of a single-row, single-column result.
async fn value(ctx: &SedonaContext, sql: &str) -> String {
    match run(ctx, sql).await {
        Ok(batches) if rows(&batches) > 0 => {
            arrow::util::display::array_value_to_string(batches[0].column(0), 0).unwrap()
        }
        Ok(_) => "<no rows>".to_string(),
        Err(err) => format!("ERROR {err}"),
    }
}

/// How many of the dataset's files the plan for `predicate` says it will open, and how the
/// predicate reached the scan.
async fn scan(ctx: &SedonaContext, predicate: &str) -> String {
    let plan = match run(
        ctx,
        &format!("EXPLAIN SELECT id FROM reading WHERE {predicate}"),
    )
    .await
    {
        Ok(batches) => arrow::util::pretty::pretty_format_batches(&batches)
            .unwrap()
            .to_string(),
        Err(err) => return format!("ERROR {err}"),
    };
    let files = plan.matches("part-0.parquet").count();
    let filter = plan
        .lines()
        .find(|line| line.contains("full_filters="))
        .map(|line| {
            line.split("full_filters=")
                .nth(1)
                .unwrap_or("")
                .trim_end_matches(['|', ' '])
                .to_string()
        })
        .unwrap_or_else(|| "<no full_filters in plan>".to_string());
    format!("{files}/3 files; pushed down as {filter}")
}

async fn report(label: &str, ctx: &SedonaContext) {
    println!(
        "[{label}] partition column type: {}",
        value(ctx, "SELECT DISTINCT arrow_typeof(ingested_date) FROM reading").await
    );
    for predicate in PREDICATES {
        let rows = run(ctx, &format!("SELECT id FROM reading WHERE {predicate}"))
            .await
            .map_or_else(|err| format!("ERROR {err}"), |b| format!("{} rows", rows(&b)));
        println!(
            "[{label}] WHERE {predicate}\n        -> {rows} of 3; scans {}",
            scan(ctx, predicate).await
        );
    }
}

#[tokio::test]
async fn spike_inferred_partition_columns() {
    let tmp = tempfile::tempdir().unwrap();
    let root = store(tmp.path()).await;
    let ctx = context(&root, GeoParquetReadOptions::default()).await;

    report("inferred", &ctx).await;
}

#[tokio::test]
async fn spike_declared_partition_columns() {
    let tmp = tempfile::tempdir().unwrap();
    let root = store(tmp.path()).await;
    let ctx = context(
        &root,
        GeoParquetReadOptions::default()
            .with_table_partition_cols(vec![("ingested_date".to_string(), DataType::Date32)]),
    )
    .await;

    report("declared", &ctx).await;
}
