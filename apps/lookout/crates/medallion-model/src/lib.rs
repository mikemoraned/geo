mod crossing;
mod motis;
mod overture;
mod session;
mod silver;
mod telemetry;

use medallion::DatasetInfo;

pub use crossing::{
    OverlapKind, SESSION_CROSSING, SessionCrossingRow, WATER_CROSSING, WaterCrossingRow,
};
pub use motis::{MOTIS_SEGMENT, MotisSegmentRow, TRAIN_SEGMENT, TrainSegmentRow};
pub use overture::{EXTRACT_MANIFEST, ExtractManifestRow, OVERTURE_EXTRACT};
pub use session::{SESSION, SESSION_SAMPLE, SessionRow, SessionSampleRow};
pub use silver::{TargetError, silver_target};
pub use telemetry::{
    ACCEL_READING, AccelReadingRow, DEVICE_SESSION, DeviceSessionRow, GPS_READING, GpsReadingRow,
    RAW_SAMPLE, RawSampleRow,
};

pub const ALL: [DatasetInfo; 12] = [
    RAW_SAMPLE.info(),
    GPS_READING.info(),
    ACCEL_READING.info(),
    DEVICE_SESSION.info(),
    MOTIS_SEGMENT.info(),
    TRAIN_SEGMENT.info(),
    SESSION.info(),
    SESSION_SAMPLE.info(),
    WATER_CROSSING.info(),
    SESSION_CROSSING.info(),
    OVERTURE_EXTRACT.info(),
    EXTRACT_MANIFEST.info(),
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
                "session",
                "session_crossing",
                "session_sample",
                "train_segment",
                "water_crossing"
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
        check_rows_of::<MotisSegmentRow>();
        check_rows_of::<TrainSegmentRow>();
        check_rows_of::<SessionRow>();
        check_rows_of::<SessionSampleRow>();
        check_rows_of::<WaterCrossingRow>();
        check_rows_of::<SessionCrossingRow>();
        check_rows_of::<ExtractManifestRow>();
    }
}
