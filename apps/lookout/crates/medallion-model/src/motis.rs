use chrono::{DateTime, NaiveDate, Utc};
use domain::TrainNumber;
use medallion::{DatasetSpec, Dated, Geometry, Row, layers};
use serde::{Deserialize, Serialize};

use crate::motis_source::MotisSourceId;

pub const MOTIS_SEGMENT: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("motis_segment", "polled_date");

pub const MOTIS_SEGMENT_V2: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("motis_segment_v2", "polled_date");

pub const TRAIN_SEGMENT: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("train_segment", "departure_date");

pub const TRAIN_SEGMENT_V2: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("train_segment_v2", "departure_date");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotisSegmentRow {
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub captured_at: DateTime<Utc>,
    pub trip_id: String,
    pub route_name: Option<String>,
    pub train_number: Option<TrainNumber>,
    pub agency_id: Option<String>,
    pub agency_name: Option<String>,
    pub mode: String,
    pub route_color: Option<String>,
    pub from_stop_id: Option<String>,
    pub from_lat: f64,
    pub from_lon: f64,
    pub to_stop_id: Option<String>,
    pub to_lat: f64,
    pub to_lon: f64,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub departure: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub arrival: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub scheduled_departure: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub scheduled_arrival: DateTime<Utc>,
    pub realtime: bool,
    pub polyline: String,
}

impl Row for MotisSegmentRow {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = MOTIS_SEGMENT;
    const INSTANTS: &'static [&'static str] = &[
        "captured_at",
        "departure",
        "arrival",
        "scheduled_departure",
        "scheduled_arrival",
    ];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotisCaptureRow {
    pub source_id: MotisSourceId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub captured_at: DateTime<Utc>,
    pub trip_id: String,
    pub route_name: Option<String>,
    pub train_number: Option<TrainNumber>,
    pub agency_id: Option<String>,
    pub agency_name: Option<String>,
    pub mode: String,
    pub route_color: Option<String>,
    pub from_stop_id: Option<String>,
    pub from_lat: f64,
    pub from_lon: f64,
    pub to_stop_id: Option<String>,
    pub to_lat: f64,
    pub to_lon: f64,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub departure: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub arrival: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub scheduled_departure: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub scheduled_arrival: DateTime<Utc>,
    pub realtime: bool,
    pub polyline: String,
}

impl MotisCaptureRow {
    pub fn of(source_id: MotisSourceId, segment: MotisSegmentRow) -> Self {
        Self {
            source_id,
            captured_at: segment.captured_at,
            trip_id: segment.trip_id,
            route_name: segment.route_name,
            train_number: segment.train_number,
            agency_id: segment.agency_id,
            agency_name: segment.agency_name,
            mode: segment.mode,
            route_color: segment.route_color,
            from_stop_id: segment.from_stop_id,
            from_lat: segment.from_lat,
            from_lon: segment.from_lon,
            to_stop_id: segment.to_stop_id,
            to_lat: segment.to_lat,
            to_lon: segment.to_lon,
            departure: segment.departure,
            arrival: segment.arrival,
            scheduled_departure: segment.scheduled_departure,
            scheduled_arrival: segment.scheduled_arrival,
            realtime: segment.realtime,
            polyline: segment.polyline,
        }
    }
}

impl Row for MotisCaptureRow {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = MOTIS_SEGMENT_V2;
    const INSTANTS: &'static [&'static str] = MotisSegmentRow::INSTANTS;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainSegmentRowV1 {
    pub trip_id: String,
    pub route_name: Option<String>,
    pub train_number: Option<TrainNumber>,
    pub agency_id: Option<String>,
    pub agency_name: Option<String>,
    pub mode: String,
    pub route_color: Option<String>,
    pub realtime: bool,
    pub from_stop_id: Option<String>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub departure: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub arrival: DateTime<Utc>,
}

impl Row for TrainSegmentRowV1 {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = TRAIN_SEGMENT;
    const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
    const INSTANTS: &'static [&'static str] = &["departure", "arrival"];
}

impl Dated for TrainSegmentRowV1 {
    fn partition_date(&self) -> NaiveDate {
        self.departure.date_naive()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainSegmentRowV2 {
    pub source_id: MotisSourceId,
    pub trip_id: String,
    pub route_name: Option<String>,
    pub train_number: Option<TrainNumber>,
    pub agency_id: Option<String>,
    pub agency_name: Option<String>,
    pub mode: String,
    pub route_color: Option<String>,
    pub realtime: bool,
    pub from_stop_id: Option<String>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub departure: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub arrival: DateTime<Utc>,
}

impl Row for TrainSegmentRowV2 {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = TRAIN_SEGMENT_V2;
    const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
    const INSTANTS: &'static [&'static str] = &["departure", "arrival"];
}

impl Dated for TrainSegmentRowV2 {
    fn partition_date(&self) -> NaiveDate {
        self.departure.date_naive()
    }
}

#[cfg(test)]
mod tests {
    use arrow::datatypes::DataType;

    use super::*;

    #[test]
    fn a_capture_carries_every_segment_column_and_its_source() {
        let segment: Vec<String> = medallion::fields::<MotisSegmentRow>()
            .expect("describe the rows")
            .iter()
            .map(|field| field.name().clone())
            .collect();
        let capture: Vec<String> = medallion::fields::<MotisCaptureRow>()
            .expect("describe the rows")
            .iter()
            .map(|field| field.name().clone())
            .collect();

        assert!(capture.contains(&"source_id".to_string()), "{capture:?}");
        for name in &segment {
            assert!(capture.contains(name), "{name} is missing from {capture:?}");
        }
        assert_eq!(capture.len(), segment.len() + 1);
    }

    #[test]
    fn a_train_number_is_a_four_byte_column_in_both_datasets() {
        for column in [
            medallion::fields::<MotisSegmentRow>()
                .expect("describe the rows")
                .iter()
                .find(|field| field.name() == "train_number")
                .expect("a train_number column")
                .data_type()
                .clone(),
            medallion::fields::<TrainSegmentRowV1>()
                .expect("describe the rows")
                .iter()
                .find(|field| field.name() == "train_number")
                .expect("a train_number column")
                .data_type()
                .clone(),
        ] {
            assert_eq!(column, DataType::UInt32);
        }
    }
}
