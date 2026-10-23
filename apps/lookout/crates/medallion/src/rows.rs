use std::sync::Arc;

use arrow::array::RecordBatch;
use arrow::datatypes::{DataType, Field, FieldRef};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_arrow::schema::{SchemaLike, TracingOptions};
use serde_json::json;

use crate::dataset::DatasetSpec;
use crate::layer::LayerKind;

const INSTANT_TYPE: &str = "Timestamp(Millisecond, Some(\"UTC\"))";

#[derive(Debug, thiserror::Error)]
pub enum RowError {
    #[error("describing the rows: {0}")]
    Schema(#[from] serde_arrow::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Geometry {
    Absent,
    LatLonAndProjected,
}

pub trait Row: Serialize + for<'de> Deserialize<'de> {
    type Layer: LayerKind;

    const DATASET: DatasetSpec<Self::Layer>;

    const GEOMETRY: Geometry = Geometry::Absent;

    const INSTANTS: &'static [&'static str] = &[];

    const UNIQUE: &'static [&'static str] = &[];

    fn example() -> Option<Self> {
        None
    }
}

pub trait Dated: Row {
    fn partition_date(&self) -> NaiveDate;
}

pub fn fields<T: Row>() -> Result<Vec<FieldRef>, RowError> {
    let options = TracingOptions::default().enums_without_data_as_strings(true);
    let options = T::INSTANTS.iter().try_fold(options, |options, &name| {
        options.overwrite(
            name,
            json!({"name": name, "data_type": INSTANT_TYPE, "nullable": true}),
        )
    })?;
    let traced = match T::example() {
        Some(example) => Vec::<FieldRef>::from_samples(&[example], options)?,
        None => Vec::<FieldRef>::from_type::<T>(options)?,
    };
    Ok(traced.iter().map(undictionary).collect())
}

fn undictionary(field: &FieldRef) -> FieldRef {
    match field.data_type() {
        DataType::Dictionary(_, values) => Arc::new(
            Field::new(field.name(), values.as_ref().clone(), field.is_nullable())
                .with_metadata(field.metadata().clone()),
        ),
        _ => field.clone(),
    }
}

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

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(transparent)]
    struct Exemplified(Reading);

    impl Row for Exemplified {
        type Layer = <Reading as Row>::Layer;
        const DATASET: DatasetSpec<Self::Layer> = Reading::DATASET;
        const INSTANTS: &'static [&'static str] = Reading::INSTANTS;

        fn example() -> Option<Self> {
            Some(Exemplified(Reading {
                name: Some("a reading".to_string()),
                ..row(1)
            }))
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(try_from = "String", into = "String")]
    struct Code(String);

    impl TryFrom<String> for Code {
        type Error = String;

        fn try_from(text: String) -> Result<Self, Self::Error> {
            if text.is_empty() {
                Err("a code cannot be empty".to_string())
            } else {
                Ok(Self(text))
            }
        }
    }

    impl From<Code> for String {
        fn from(code: Code) -> Self {
            code.0
        }
    }

    #[derive(Debug, Serialize, Deserialize)]
    struct Coded {
        id: i64,
        code: Option<Code>,
    }

    impl Row for Coded {
        type Layer = crate::layer::layers::Bronze;
        const DATASET: DatasetSpec<Self::Layer> = DatasetSpec::unpartitioned("coded");

        fn example() -> Option<Self> {
            Some(Coded {
                id: 1,
                code: Some(Code("a".to_string())),
            })
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

    #[test]
    fn other_integer_columns_are_left_alone() {
        let fields = fields::<Reading>().unwrap();

        let id = fields.iter().find(|f| f.name() == "id").unwrap();
        assert_eq!(id.data_type(), &DataType::Int64);
    }

    #[test]
    fn a_row_traced_from_its_example_has_the_columns_its_type_gives() {
        assert_eq!(
            fields::<Exemplified>().unwrap(),
            fields::<Reading>().unwrap()
        );
    }

    #[test]
    fn a_row_holding_a_value_that_refuses_an_empty_string_is_traced_from_its_example() {
        let fields = fields::<Coded>().unwrap();

        let code = fields.iter().find(|f| f.name() == "code").unwrap();
        assert!(
            matches!(code.data_type(), DataType::Utf8 | DataType::LargeUtf8),
            "{code:?}"
        );
        assert!(code.is_nullable(), "{code:?}");
    }

    #[test]
    fn a_value_that_refuses_an_empty_string_still_checks_what_is_read_back() {
        let batch = batch(&[Coded {
            id: 1,
            code: Some(Code(String::new())),
        }])
        .unwrap();

        assert!(serde_arrow::from_record_batch::<Vec<Coded>>(&batch).is_err());
    }

    #[test]
    fn rows_become_a_batch_of_the_same_length() {
        let batch = batch(&[row(1), row(2)]).unwrap();

        assert_eq!(batch.num_rows(), 2);
        assert_eq!(batch.num_columns(), 4);
    }

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
