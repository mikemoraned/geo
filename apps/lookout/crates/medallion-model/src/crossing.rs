use chrono::{DateTime, NaiveDate, Utc};
use domain::{CrossingCompactId, CrossingId, DeviceId, SessionId};
use medallion::{COUNTRY, DatasetSpec, Dated, Geometry, Row, layers};
use serde::{Deserialize, Serialize};

use crate::SessionRow;

pub const WATER_CROSSING_V2: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("water_crossing_v2", COUNTRY);

pub const SESSION_CROSSING: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("session_crossing", "crossed_date");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlapKind {
    Line,
    Point,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaterCrossingRowV2 {
    pub crossing_id: CrossingId,
    pub crossing_compact_id: CrossingCompactId,
    pub overture_water_id: String,
    pub overture_water_subtype: Option<String>,
    pub overture_water_class: Option<String>,
    pub track_id: String,
    pub overture_rail_id: String,
    pub overture_rail_class: Option<String>,
    pub overlap_kind: OverlapKind,
    pub overlap_m: f64,
    pub total_overlap_m: f64,
    pub merged_parts: u32,
    pub frac: f64,
    pub extract_id: String,
    pub merge_distance_m: f64,
    pub min_crossing_m: f64,
}

impl Row for WaterCrossingRowV2 {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = WATER_CROSSING_V2;
    const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
    const UNIQUE: &'static [&'static str] = &["crossing_id", "crossing_compact_id"];

    fn example() -> Self {
        Self {
            crossing_id: CrossingId::new("example").expect("a constant crossing id is a name"),
            crossing_compact_id: CrossingCompactId::new(1),
            overture_water_id: "example".to_string(),
            overture_water_subtype: Some("example".to_string()),
            overture_water_class: Some("example".to_string()),
            track_id: "example".to_string(),
            overture_rail_id: "example".to_string(),
            overture_rail_class: Some("example".to_string()),
            overlap_kind: OverlapKind::Line,
            overlap_m: 0.0,
            total_overlap_m: 0.0,
            merged_parts: 1,
            frac: 0.0,
            extract_id: "example".to_string(),
            merge_distance_m: 0.0,
            min_crossing_m: 0.0,
        }
    }
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

    fn example() -> Self {
        let session = SessionRow::example();
        Self {
            session_id: session.session_id,
            crossing_id: WaterCrossingRowV2::example().crossing_id,
            device_id: session.device_id,
            crossed_at: session.started_at,
            distance_m: 0.0,
            samples_within: 1,
            match_radius_m: 250.0,
        }
    }
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
    use crate::testing::column;

    #[test]
    fn a_compact_id_is_a_four_byte_column() {
        assert_eq!(
            column::<WaterCrossingRowV2>("crossing_compact_id").data_type(),
            &DataType::UInt32
        );
    }

    #[test]
    fn a_crossing_id_is_a_string_column_in_both_datasets() {
        assert!(matches!(
            column::<WaterCrossingRowV2>("crossing_id").data_type(),
            DataType::Utf8 | DataType::LargeUtf8
        ));
        assert!(matches!(
            column::<SessionCrossingRow>("crossing_id").data_type(),
            DataType::Utf8 | DataType::LargeUtf8
        ));
    }

    #[test]
    fn the_kind_of_overlap_is_a_string_column() {
        assert!(matches!(
            column::<WaterCrossingRowV2>("overlap_kind").data_type(),
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
            assert_eq!(
                column::<WaterCrossingRowV2>(tuning).data_type(),
                &DataType::Float64
            );
        }
        assert_eq!(
            column::<SessionCrossingRow>("match_radius_m").data_type(),
            &DataType::Float64
        );
    }
}
