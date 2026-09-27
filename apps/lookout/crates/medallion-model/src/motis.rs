use chrono::{DateTime, NaiveDate, Utc};
use domain::TrainNumber;
use medallion::{DatasetSpec, Dated, Geometry, Row, layers};
use serde::{Deserialize, Serialize};

pub const MOTIS_SEGMENT: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("motis_segment", "polled_date");

pub const TRAIN_SEGMENT: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("train_segment", "departure_date");

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
pub struct TrainSegmentRow {
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

impl Row for TrainSegmentRow {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = TRAIN_SEGMENT;
    const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
    const INSTANTS: &'static [&'static str] = &["departure", "arrival"];
}

impl Dated for TrainSegmentRow {
    fn partition_date(&self) -> NaiveDate {
        self.departure.date_naive()
    }
}

#[cfg(test)]
mod tests {
    use arrow::datatypes::DataType;

    use super::*;

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
            medallion::fields::<TrainSegmentRow>()
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
