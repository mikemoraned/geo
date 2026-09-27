use chrono::{DateTime, NaiveDate, Utc};
use domain::{CrossingCompactId, CrossingId, DeviceId, SessionId};
use medallion::{COUNTRY, DatasetSpec, Dated, Geometry, Row, layers};
use serde::{Deserialize, Serialize};

pub const WATER_CROSSING: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("water_crossing", COUNTRY);

pub const SESSION_CROSSING: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("session_crossing", "crossed_date");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlapKind {
    Line,
    Point,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaterCrossingRow {
    pub crossing_id: CrossingId,
    pub crossing_compact_id: CrossingCompactId,
    pub water_id: String,
    pub water_subtype: Option<String>,
    pub water_class: Option<String>,
    pub track_id: String,
    pub rail_id: String,
    pub rail_class: Option<String>,
    pub overlap_kind: OverlapKind,
    pub overlap_m: f64,
    pub total_overlap_m: f64,
    pub merged_parts: u32,
    pub frac: f64,
    pub extract_id: String,
    pub merge_distance_m: f64,
    pub min_crossing_m: f64,
}

impl Row for WaterCrossingRow {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = WATER_CROSSING;
    const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
    const UNIQUE: &'static [&'static str] = &["crossing_id", "crossing_compact_id"];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionCrossingRow {
    pub session_id: SessionId,
    pub crossing_id: CrossingId,
    pub device_id: DeviceId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub crossed_at: DateTime<Utc>,
    pub distance_m: f64,
    pub samples_within: u32,
    pub match_radius_m: f64,
}

impl Row for SessionCrossingRow {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = SESSION_CROSSING;
    const INSTANTS: &'static [&'static str] = &["crossed_at"];
}

impl Dated for SessionCrossingRow {
    fn partition_date(&self) -> NaiveDate {
        self.crossed_at.date_naive()
    }
}

#[cfg(test)]
mod tests {
    use arrow::datatypes::DataType;

    use super::*;

    fn column<T: Row>(name: &str) -> DataType {
        medallion::fields::<T>()
            .expect("describe the rows")
            .iter()
            .find(|field| field.name() == name)
            .unwrap_or_else(|| panic!("no column named `{name}`"))
            .data_type()
            .clone()
    }

    #[test]
    fn a_compact_id_is_a_four_byte_column() {
        assert_eq!(
            column::<WaterCrossingRow>("crossing_compact_id"),
            DataType::UInt32
        );
    }

    #[test]
    fn a_crossing_id_is_a_string_column_in_both_datasets() {
        assert!(matches!(
            column::<WaterCrossingRow>("crossing_id"),
            DataType::Utf8 | DataType::LargeUtf8
        ));
        assert!(matches!(
            column::<SessionCrossingRow>("crossing_id"),
            DataType::Utf8 | DataType::LargeUtf8
        ));
    }

    #[test]
    fn the_kind_of_overlap_is_a_string_column() {
        assert!(matches!(
            column::<WaterCrossingRow>("overlap_kind"),
            DataType::Utf8 | DataType::LargeUtf8
        ));
    }

    #[test]
    fn any_id_can_name_a_partition() {
        let id: CrossingId = "08b2a5c1fffffff-08f2a5c1".parse().expect("a name");

        assert!(medallion::PartitionValue::new(id.to_string()).is_ok());
    }

    #[test]
    fn a_crossing_carries_how_it_was_collapsed() {
        for tuning in ["merge_distance_m", "min_crossing_m"] {
            assert_eq!(column::<WaterCrossingRow>(tuning), DataType::Float64);
        }
        assert_eq!(
            column::<SessionCrossingRow>("match_radius_m"),
            DataType::Float64
        );
    }
}
