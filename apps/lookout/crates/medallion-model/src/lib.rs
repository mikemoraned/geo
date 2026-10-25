mod bronze;
mod crossing;
#[cfg(test)]
mod declaration;
mod gers;
mod motis;
mod motis_source;
mod overture;
mod overture_release;
mod session;
mod silver;
mod telemetry;
mod testing;
mod train_session;

use medallion::DatasetInfo;

pub use bronze::{NoSuchBronzeDataset, bronze_dataset};
pub use crossing::{
    OverlapKind, SESSION_CROSSING_V2, SessionCrossingRowV2, WATER_CROSSING_V2, WaterCrossingRowV2,
};
pub use gers::{GersId, NotAGersId};
pub use motis::{
    AgencyId, EmptyAgencyId, EmptyStopId, EmptyTripId, MOTIS_SEGMENT, MOTIS_SEGMENT_V2,
    MotisCaptureRow, MotisSegmentRow, RailMode, StopId, TRAIN_SEGMENT_V2, TrainSegmentRowV2,
    TripId,
};
pub use motis_source::{
    Area, Feed, Label, MOTIS_SOURCE_V2, MotisSource, MotisSourceId, MotisSourceRowV2, MotisVersion,
    NotALabel, NotAMotisSource, NotAMotisVersion, UnknownFeed,
};
pub use overture::{
    DIVISION_ID, EXTRACT_MANIFEST_V2, ExtractManifestRowV2, OVERTURE_EXTRACT, OvertureRegion,
    division_id,
};
pub use overture_release::{NotAnOvertureRelease, OvertureRelease};
pub use session::{SESSION_SAMPLE_V2, SESSION_V2, SessionRowV2, SessionSampleRowV2};
pub use silver::{TargetError, silver_target};
pub use telemetry::{
    ACCEL_READING, ACCEL_READING_V2, AccelReadingRow, AccelReadingRowV2, DEVICE_SESSION,
    DEVICE_SESSION_V2, DeviceSessionRow, DeviceSessionRowV2, GPS_READING, GPS_READING_V2,
    GpsReadingRow, GpsReadingRowV2, RAW_SAMPLE, RawSampleRow,
};
pub use train_session::{
    GeometrySource, TRAIN_SESSION, TRAIN_SESSION_SAMPLE, TrainSessionRow, TrainSessionSampleRow,
    train_session_id,
};

pub const ALL: [DatasetInfo; 19] = [
    RAW_SAMPLE.info(),
    GPS_READING.info(),
    ACCEL_READING.info(),
    DEVICE_SESSION.info(),
    GPS_READING_V2.info(),
    ACCEL_READING_V2.info(),
    DEVICE_SESSION_V2.info(),
    MOTIS_SEGMENT.info(),
    MOTIS_SEGMENT_V2.info(),
    MOTIS_SOURCE_V2.info(),
    TRAIN_SEGMENT_V2.info(),
    SESSION_V2.info(),
    SESSION_SAMPLE_V2.info(),
    TRAIN_SESSION.info(),
    TRAIN_SESSION_SAMPLE.info(),
    WATER_CROSSING_V2.info(),
    SESSION_CROSSING_V2.info(),
    OVERTURE_EXTRACT.info(),
    EXTRACT_MANIFEST_V2.info(),
];

#[cfg(test)]
mod tests {
    use arrow::datatypes::DataType;
    use medallion::Row;

    use super::*;

    #[test]
    fn dataset_names_are_unique() {
        let mut names: Vec<&str> = ALL.iter().map(|dataset| dataset.name).collect();
        names.sort_unstable();
        let unique = names.len();
        names.dedup();

        assert_eq!(
            names.len(),
            unique,
            "duplicate dataset name in {names:?}: two datasets named alike share a directory, \
             and merge into one without saying so"
        );
    }

    #[test]
    fn only_the_derived_datasets_may_be_replaced() {
        let mut replaceable: Vec<&str> = ALL
            .iter()
            .filter(|dataset| dataset.layer.permits_replacement())
            .map(|dataset| dataset.name)
            .collect();
        replaceable.sort_unstable();

        assert_eq!(
            replaceable,
            [
                "session_crossing_v2",
                "session_sample_v2",
                "session_v2",
                "train_segment_v2",
                "train_session",
                "train_session_sample",
                "water_crossing_v2"
            ]
        );
    }

    fn partition_keys() -> Vec<(&'static str, &'static str)> {
        ALL.iter()
            .filter_map(|dataset| Some((dataset.name, dataset.partition_key?)))
            .collect()
    }

    #[test]
    fn every_name_and_partition_key_is_snake_case() {
        for dataset in ALL {
            assert!(
                dataset.name.parse::<medallion::PartitionKey>().is_ok(),
                "{}: dataset name is not snake_case, and nothing else checks it until a path is \
                 built from it",
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

    #[test]
    fn a_date_valued_partition_key_names_the_event_it_records() {
        for (name, key) in partition_keys() {
            assert!(
                !key.contains("date") || key.ends_with("_date"),
                "{name}: date partition key `{key}` should be named `<event>_date`"
            );
        }
    }

    fn check_rows_of<T: Row>() {
        assert!(
            ALL.contains(&T::DATASET.info()),
            "{} is not among the datasets defined here",
            T::DATASET.name
        );

        let fields = medallion::fields::<T>().expect("describe the rows");
        for instant in T::INSTANTS {
            let field = fields
                .iter()
                .find(|field| field.name() == instant)
                .unwrap_or_else(|| {
                    panic!(
                        "{}: no column named `{instant}`, so nothing would convert it and it \
                         would be stored as the integer it travels as",
                        T::DATASET.name
                    )
                });
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
        check_rows_of::<GpsReadingRowV2>();
        check_rows_of::<AccelReadingRowV2>();
        check_rows_of::<DeviceSessionRowV2>();
        check_rows_of::<MotisSegmentRow>();
        check_rows_of::<MotisCaptureRow>();
        check_rows_of::<MotisSourceRowV2>();
        check_rows_of::<TrainSegmentRowV2>();
        check_rows_of::<SessionRowV2>();
        check_rows_of::<SessionSampleRowV2>();
        check_rows_of::<TrainSessionRow>();
        check_rows_of::<TrainSessionSampleRow>();
        check_rows_of::<WaterCrossingRowV2>();
        check_rows_of::<SessionCrossingRowV2>();
        check_rows_of::<ExtractManifestRowV2>();
    }
}
