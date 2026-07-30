//! Turning plain Rust rows into the arrow batches the store writes.
//!
//! A dataset's rows are one serde type implementing [`Row`], which carries the dataset the
//! rows belong to and which of its columns hold an instant. A writer therefore states a
//! dataset's schema by naming its row type, and a reader of a dataset it did not write has
//! the same type to read it back through.
//!
//! **Instants are UTC millisecond timestamps.** They travel through serde as epoch
//! milliseconds — an integer carrying no unit or timezone of its own — so a row type names
//! its instant columns and they are declared as timestamps here. This is the one place
//! that rule is expressed, so datasets cannot drift apart on the representation of time.
//!
//! **A variant is stored as its name.** A column whose Rust type is an enum of dataless
//! variants is a string column rather than a union, since engines vary in what they make
//! of a union and a stored dataset must not depend on which one reads it.

use std::sync::Arc;

use arrow::array::RecordBatch;
use arrow::datatypes::{DataType, Field, FieldRef};
use serde::{Deserialize, Serialize};
use serde_arrow::schema::{SchemaLike, TracingOptions};
use serde_json::json;

use crate::dataset::DatasetSpec;
use crate::layer::LayerKind;

/// How an instant column is stored, whatever integer the row type carries it as.
const INSTANT_TYPE: &str = "Timestamp(Millisecond, Some(\"UTC\"))";

/// Failure describing or building a dataset's rows.
#[derive(Debug, thiserror::Error)]
pub enum RowError {
    #[error("describing the rows: {0}")]
    Schema(#[from] serde_arrow::Error),
}

/// One dataset's rows: the columns it holds, and where those rows live.
///
/// A dataset whose rows carry geometry declares only its other columns here, since a
/// geometry column is built as arrow rather than traced from a Rust type; the writer
/// appends it to [`fields`]' output.
pub trait Row: Serialize + for<'de> Deserialize<'de> {
    /// The layer the dataset lives in, which decides what may be done to it.
    type Layer: LayerKind;

    /// The dataset these rows make up.
    const DATASET: DatasetSpec<Self::Layer>;

    /// The columns holding an instant, declared as timestamps rather than left as the
    /// integers they travel through serde as.
    const INSTANTS: &'static [&'static str] = &[];
}

/// The arrow schema of `T`, with its instant columns declared as timestamps.
///
/// Callers that append geometry columns need the fields separately; those writing rows
/// alone can use [`batch`] instead.
pub fn fields<T: Row>() -> Result<Vec<FieldRef>, RowError> {
    let options = TracingOptions::default().enums_without_data_as_strings(true);
    let options = T::INSTANTS.iter().try_fold(options, |options, &name| {
        options.overwrite(
            name,
            json!({"name": name, "data_type": INSTANT_TYPE, "nullable": true}),
        )
    })?;
    Ok(Vec::<FieldRef>::from_type::<T>(options)?
        .iter()
        .map(undictionary)
        .collect())
}

/// A dictionary-encoded field as a plain field of its values.
///
/// Tracing a dataless enum yields a dictionary of its variant names. The encoding is a
/// storage decision the parquet writer makes per column anyway, so it is dropped from the
/// schema rather than being carried into it and read back differently by each engine.
fn undictionary(field: &FieldRef) -> FieldRef {
    match field.data_type() {
        DataType::Dictionary(_, values) => Arc::new(
            Field::new(field.name(), values.as_ref().clone(), field.is_nullable())
                .with_metadata(field.metadata().clone()),
        ),
        _ => field.clone(),
    }
}

/// One batch holding `rows`.
pub fn batch<T: Row>(rows: &[T]) -> Result<RecordBatch, RowError> {
    Ok(serde_arrow::to_record_batch(&fields::<T>()?, &rows)?)
}

#[cfg(test)]
mod tests {
    use arrow::datatypes::TimeUnit;
    use chrono::{DateTime, Utc};

    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    struct Reading {
        id: i64,
        t: i64,
        name: Option<String>,
        source: Source,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(rename_all = "snake_case")]
    enum Source {
        Measured,
        Inferred,
    }

    impl Row for Reading {
        type Layer = crate::layer::layers::Bronze;
        const DATASET: DatasetSpec<Self::Layer> =
            DatasetSpec::partitioned("reading", "ingested_date");
        const INSTANTS: &'static [&'static str] = &["t"];
    }

    fn row(id: i64) -> Reading {
        Reading {
            id,
            t: 1_700_000_000_000,
            name: None,
            source: Source::Measured,
        }
    }

    #[test]
    fn a_named_instant_column_is_typed_as_a_utc_millisecond_timestamp() {
        let fields = fields::<Reading>().unwrap();

        let t = fields.iter().find(|f| f.name() == "t").unwrap();
        assert_eq!(
            t.data_type(),
            &DataType::Timestamp(TimeUnit::Millisecond, Some("UTC".into()))
        );
    }

    /// A column the row type does not name as an instant keeps the type its Rust field
    /// has, so the rule applies only where the row type asks for it.
    #[test]
    fn other_integer_columns_are_left_alone() {
        let fields = fields::<Reading>().unwrap();

        let id = fields.iter().find(|f| f.name() == "id").unwrap();
        assert_eq!(id.data_type(), &DataType::Int64);
    }

    #[test]
    fn rows_become_a_batch_of_the_same_length() {
        let batch = batch(&[row(1), row(2)]).unwrap();

        assert_eq!(batch.num_rows(), 2);
        assert_eq!(batch.num_columns(), 4);
    }

    /// A union is read differently by different engines, so a dataless variant is stored
    /// as its name instead.
    #[test]
    fn a_column_of_dataless_variants_is_stored_as_a_string() {
        let fields = fields::<Reading>().unwrap();

        let source = fields.iter().find(|f| f.name() == "source").unwrap();
        assert!(
            matches!(source.data_type(), DataType::Utf8 | DataType::LargeUtf8),
            "expected a string column, got {:?}",
            source.data_type()
        );
    }

    #[test]
    fn a_variant_round_trips_through_a_batch() {
        let batch = batch(&[row(1)]).unwrap();

        let rows: Vec<Reading> = serde_arrow::from_record_batch(&batch).unwrap();
        assert_eq!(rows[0].source, Source::Measured);
    }

    /// The timestamps survive as instants, rather than being reinterpreted as some other
    /// unit on the way in.
    #[test]
    fn an_instant_round_trips_through_a_batch() {
        let batch = batch(&[row(1)]).unwrap();

        let rows: Vec<Reading> = serde_arrow::from_record_batch(&batch).unwrap();
        assert_eq!(
            DateTime::from_timestamp_millis(rows[0].t),
            DateTime::<Utc>::from_timestamp_millis(1_700_000_000_000)
        );
    }
}
