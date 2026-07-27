//! The datasets lookout holds, and where each one lives in the store.
//!
//! Every dataset is defined once here, so a writer and a reader of the same data agree on
//! its layer and partitioning by referring to the same value rather than by each spelling
//! out a name and a key. `docs/medallion.md` describes the layout in prose; this is its
//! executable form.

use medallion::{DatasetSpec, Layer};

/// Every payload the telemetry queue carried, verbatim. The lossless record the other
/// telemetry datasets are interpreted from.
pub const RAW_SAMPLE: DatasetSpec =
    DatasetSpec::partitioned(Layer::Bronze, "raw_sample", "ingested_date");

/// GPS fixes interpreted from the payloads.
pub const GPS_READING: DatasetSpec =
    DatasetSpec::partitioned(Layer::Bronze, "gps_reading", "ingested_date");

/// Accelerometer readings interpreted from the payloads.
pub const ACCEL_READING: DatasetSpec =
    DatasetSpec::partitioned(Layer::Bronze, "accel_reading", "ingested_date");

/// The metadata a device announces when it starts a session.
pub const DEVICE_SESSION: DatasetSpec =
    DatasetSpec::partitioned(Layer::Bronze, "device_session", "ingested_date");

/// Trip segments as polled from the transit service, duplication allowed.
pub const MOTIS_SEGMENT: DatasetSpec =
    DatasetSpec::partitioned(Layer::Bronze, "motis_segment", "polled_date");

/// One row per scheduled leg, deduped from the polled segments and carrying its geometry.
pub const TRAIN_SEGMENT: DatasetSpec =
    DatasetSpec::partitioned(Layer::Silver, "train_segment", "departure_date");

/// Overture Maps rows as extracted, in Overture's own shape and directory layout below
/// the id of the extraction that fetched them.
pub const OVERTURE_EXTRACT: DatasetSpec =
    DatasetSpec::partitioned(Layer::Bronze, "overture_extract", "extract_id");

/// One row per extraction: what it fetched, from which release, and when. The provenance
/// [`OVERTURE_EXTRACT`]'s rows carry only the id of.
pub const EXTRACT_MANIFEST: DatasetSpec =
    DatasetSpec::unpartitioned(Layer::Bronze, "extract_manifest");

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
}
