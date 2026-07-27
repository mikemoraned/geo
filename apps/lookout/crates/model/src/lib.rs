//! The datasets lookout holds: where each one lives in the store, and what columns it has.
//!
//! Every dataset is defined once here, so a writer and a reader of the same data agree on
//! its layer, partitioning and shape by referring to the same values rather than by each
//! spelling out a name, a key and a struct of its own. `docs/medallion.md` describes the
//! layout in prose; this is its executable form.
//!
//! A dataset's columns are a [`medallion::Row`] type declared beside its
//! [`medallion::DatasetSpec`]. Geometry columns are the exception: they are built as arrow
//! rather than traced from a Rust type, so a row type declares the dataset's other columns
//! and the writer appends [`medallion::GEOMETRY`] and [`medallion::PROJECTED_GEOMETRY`] to
//! them.

mod motis;
mod overture;
mod telemetry;

use medallion::DatasetSpec;

pub use motis::{MotisSegmentRow, TrainSegmentRow, MOTIS_SEGMENT, TRAIN_SEGMENT};
pub use overture::{ExtractManifestRow, EXTRACT_MANIFEST, OVERTURE_EXTRACT};
pub use telemetry::{
    AccelReadingRow, DeviceSessionRow, GpsReadingRow, RawSampleRow, ACCEL_READING, DEVICE_SESSION,
    GPS_READING, RAW_SAMPLE,
};

/// Every dataset defined here, for checks that must cover all of them.
pub const ALL: [DatasetSpec; 8] = [
    RAW_SAMPLE,
    GPS_READING,
    ACCEL_READING,
    DEVICE_SESSION,
    MOTIS_SEGMENT,
    TRAIN_SEGMENT,
    OVERTURE_EXTRACT,
    EXTRACT_MANIFEST,
];

#[cfg(test)]
mod tests {
    use arrow::datatypes::DataType;
    use medallion::Row;

    use super::*;

    /// Two datasets sharing a name would share a directory, and so silently merge.
    #[test]
    fn dataset_names_are_unique() {
        let mut names: Vec<&str> = ALL.iter().map(|dataset| dataset.name).collect();
        names.sort_unstable();
        let unique = names.len();
        names.dedup();

        assert_eq!(names.len(), unique, "duplicate dataset name in {names:?}");
    }

    /// The datasets that declare a partition key, as `(dataset name, key)`.
    fn partition_keys() -> Vec<(&'static str, &'static str)> {
        ALL.iter()
            .filter_map(|dataset| Some((dataset.name, dataset.partition_key?)))
            .collect()
    }

    /// Names and keys have to meet the store's naming rules, which are otherwise only
    /// checked when a path is built — at which point a bad definition is a runtime error.
    #[test]
    fn every_name_and_partition_key_is_valid() {
        for dataset in ALL {
            assert!(
                dataset.name.parse::<medallion::PartitionKey>().is_ok(),
                "{}: dataset name is not snake_case",
                dataset.name
            );
        }
        for (name, key) in partition_keys() {
            assert!(
                key.parse::<medallion::PartitionKey>().is_ok(),
                "{name}: partition key `{key}` is not snake_case"
            );
        }
    }

    /// A date-valued key is named for the event it dates, never a bare `date`. Keys of
    /// other kinds — an id, a region — are named for what they hold instead.
    #[test]
    fn a_date_valued_partition_key_names_the_event_it_records() {
        for (name, key) in partition_keys() {
            assert!(
                !key.contains("date") || key.ends_with("_date"),
                "{name}: date partition key `{key}` should be named `<event>_date`"
            );
        }
    }

    /// A row type describes a dataset defined here, and every column it calls an instant
    /// is a column it has — a name matching nothing would leave that column silently
    /// stored as the integer it travels as.
    fn check_rows_of<T: Row>() {
        assert!(
            ALL.contains(&T::DATASET),
            "{} is not among the datasets defined here",
            T::DATASET.name
        );

        let fields = medallion::fields::<T>().expect("describe the rows");
        for instant in T::INSTANTS {
            let field = fields
                .iter()
                .find(|field| field.name() == instant)
                .unwrap_or_else(|| panic!("{}: no column named `{instant}`", T::DATASET.name));
            assert!(
                matches!(field.data_type(), DataType::Timestamp(..)),
                "{}: `{instant}` is {:?}, not a timestamp",
                T::DATASET.name,
                field.data_type()
            );
        }
    }

    #[test]
    fn every_row_type_describes_a_dataset_and_names_its_own_instant_columns() {
        check_rows_of::<RawSampleRow>();
        check_rows_of::<GpsReadingRow>();
        check_rows_of::<AccelReadingRow>();
        check_rows_of::<DeviceSessionRow>();
        check_rows_of::<MotisSegmentRow>();
        check_rows_of::<TrainSegmentRow>();
        check_rows_of::<ExtractManifestRow>();
    }
}
