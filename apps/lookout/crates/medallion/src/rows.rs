//! Turning plain Rust rows into the arrow batches the store writes.
//!
//! Rows are defined once as a serde type and converted here, so a dataset's schema is its
//! Rust type rather than a set of hand-built column builders.
//!
//! **Instants are UTC millisecond timestamps.** They travel through serde as epoch
//! milliseconds — an integer carrying no unit or timezone of its own — so every writer
//! names its instant columns and they are declared as timestamps here. This is the one
//! place that rule is expressed, so datasets cannot drift apart on the representation of
//! time.

use arrow::array::RecordBatch;
use arrow::datatypes::FieldRef;
use serde::{Deserialize, Serialize};
use serde_arrow::schema::{SchemaLike, TracingOptions};
use serde_json::json;

/// How an instant column is stored, whatever integer the row type carries it as.
const INSTANT_TYPE: &str = "Timestamp(Millisecond, Some(\"UTC\"))";

/// Failure describing or building a dataset's rows.
#[derive(Debug, thiserror::Error)]
pub enum RowError {
    #[error("describing the rows: {0}")]
    Schema(#[from] serde_arrow::Error),
}

/// The arrow schema of `T`, with `instants` declared as timestamp columns.
///
/// Callers that append geometry columns need the fields separately; those writing rows
/// alone can use [`batch`] instead.
pub fn fields<T>(instants: &[&str]) -> Result<Vec<FieldRef>, RowError>
where
    T: for<'de> Deserialize<'de>,
{
    let options = instants
        .iter()
        .try_fold(TracingOptions::default(), |options, &name| {
            options.overwrite(
                name,
                json!({"name": name, "data_type": INSTANT_TYPE, "nullable": true}),
            )
        })?;
    Ok(Vec::<FieldRef>::from_type::<T>(options)?)
}

/// One batch holding `rows`, with `instants` declared as timestamp columns.
pub fn batch<T>(rows: &[T], instants: &[&str]) -> Result<RecordBatch, RowError>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    Ok(serde_arrow::to_record_batch(
        &fields::<T>(instants)?,
        &rows,
    )?)
}

#[cfg(test)]
mod tests {
    use arrow::datatypes::{DataType, TimeUnit};
    use chrono::{DateTime, Utc};

    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    struct Row {
        id: i64,
        t: i64,
        name: Option<String>,
    }

    fn row(id: i64) -> Row {
        Row {
            id,
            t: 1_700_000_000_000,
            name: None,
        }
    }

    #[test]
    fn a_named_instant_column_is_typed_as_a_utc_millisecond_timestamp() {
        let fields = fields::<Row>(&["t"]).unwrap();

        let t = fields.iter().find(|f| f.name() == "t").unwrap();
        assert_eq!(
            t.data_type(),
            &DataType::Timestamp(TimeUnit::Millisecond, Some("UTC".into()))
        );
    }

    /// A column not named as an instant keeps the type its Rust field has, so the rule
    /// applies only where a writer asks for it.
    #[test]
    fn other_integer_columns_are_left_alone() {
        let fields = fields::<Row>(&["t"]).unwrap();

        let id = fields.iter().find(|f| f.name() == "id").unwrap();
        assert_eq!(id.data_type(), &DataType::Int64);
    }

    #[test]
    fn rows_become_a_batch_of_the_same_length() {
        let batch = batch(&[row(1), row(2)], &["t"]).unwrap();

        assert_eq!(batch.num_rows(), 2);
        assert_eq!(batch.num_columns(), 3);
    }

    /// The timestamps survive as instants, rather than being reinterpreted as some other
    /// unit on the way in.
    #[test]
    fn an_instant_round_trips_through_a_batch() {
        let batch = batch(&[row(1)], &["t"]).unwrap();

        let rows: Vec<Row> = serde_arrow::from_record_batch(&batch).unwrap();
        assert_eq!(
            DateTime::from_timestamp_millis(rows[0].t),
            DateTime::<Utc>::from_timestamp_millis(1_700_000_000_000)
        );
    }
}
