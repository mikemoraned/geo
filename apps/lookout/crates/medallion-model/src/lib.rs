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
    OverlapKind, SESSION_CROSSING_V3, SessionCrossingRowV3, WATER_CROSSING_V2, WaterCrossingRowV2,
};
pub use gers::{GersId, NotAGersId};
pub use motis::{
    AgencyId, EmptyAgencyId, EmptyStopId, EmptyTripId, MOTIS_POLL_V1, MOTIS_POLYLINE_V1,
    MOTIS_SEGMENT, MOTIS_SEGMENT_V3, MotisPollRowV1, MotisPolylineRowV1, MotisSegmentRow,
    MotisSegmentRowV3, NotAPolylineId, NotAnEncodedPolyline, Polyline, PolylineId, RailMode,
    StopId, TRAIN_SEGMENT_V4, TrainSegmentRowV4, TripId,
};
pub use motis_source::{
    Area, Feed, Label, MOTIS_SOURCE_V3, MOTIS_SOURCE_V4, MotisBaseUrl, MotisSource, MotisSourceId,
    MotisSourceRowV3, MotisSourceRowV4, MotisVersion, NotALabel, NotAMotisBaseUrl,
    NotAMotisVersion, UnknownFeed,
};
pub use overture::{
    DIVISION_ID, EXTRACT_MANIFEST_V3, ExtractManifestRowV3, OVERTURE_EXTRACT, OvertureRegion,
    division_id,
};
pub use overture_release::{NotAnOvertureRelease, OvertureRelease};
pub use session::{SESSION_SAMPLE_V3, SESSION_V3, SessionRowV3, SessionSampleRowV3};
pub use silver::{TargetError, silver_target};
pub use telemetry::{
    ACCEL_READING_V2, AccelReadingRowV2, DEVICE_SESSION_V2, DeviceSessionRowV2, GPS_READING_V2,
    GpsReadingRowV2, RAW_SAMPLE, RawSampleRow,
};
pub use train_session::{
    GeometrySource, TRAIN_SESSION_SAMPLE_V2, TRAIN_SESSION_V2, TrainSessionRowV2,
    TrainSessionSampleRowV2, train_session_id,
};

pub const ALL: [DatasetInfo; 19] = [
    RAW_SAMPLE.info(),
    GPS_READING_V2.info(),
    ACCEL_READING_V2.info(),
    DEVICE_SESSION_V2.info(),
    MOTIS_POLL_V1.info(),
    MOTIS_SEGMENT.info(),
    MOTIS_SEGMENT_V3.info(),
    MOTIS_POLYLINE_V1.info(),
    MOTIS_SOURCE_V3.info(),
    MOTIS_SOURCE_V4.info(),
    TRAIN_SEGMENT_V4.info(),
    SESSION_V3.info(),
    SESSION_SAMPLE_V3.info(),
    TRAIN_SESSION_V2.info(),
    TRAIN_SESSION_SAMPLE_V2.info(),
    WATER_CROSSING_V2.info(),
    SESSION_CROSSING_V3.info(),
    OVERTURE_EXTRACT.info(),
    EXTRACT_MANIFEST_V3.info(),
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
                "session_crossing_v3",
                "session_sample_v3",
                "session_v3",
                "train_segment_v4",
                "train_session_sample_v2",
                "train_session_v2",
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
        check_rows_of::<GpsReadingRowV2>();
        check_rows_of::<AccelReadingRowV2>();
        check_rows_of::<DeviceSessionRowV2>();
        check_rows_of::<MotisSegmentRow>();
        check_rows_of::<MotisSegmentRowV3>();
        check_rows_of::<MotisPollRowV1>();
        check_rows_of::<MotisPolylineRowV1>();
        check_rows_of::<MotisSourceRowV3>();
        check_rows_of::<MotisSourceRowV4>();
        check_rows_of::<TrainSegmentRowV4>();
        check_rows_of::<SessionRowV3>();
        check_rows_of::<SessionSampleRowV3>();
        check_rows_of::<TrainSessionRowV2>();
        check_rows_of::<TrainSessionSampleRowV2>();
        check_rows_of::<WaterCrossingRowV2>();
        check_rows_of::<SessionCrossingRowV3>();
        check_rows_of::<ExtractManifestRowV3>();
    }
}
