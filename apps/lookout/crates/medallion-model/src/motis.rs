//! The transit datasets: segments as polled, and the scheduled legs derived from them.

use chrono::{DateTime, NaiveDate, Utc};
use domain::TrainNumber;
use medallion::{DatasetSpec, Dated, Geometry, Row, layers};
use serde::{Deserialize, Serialize};

/// Trip segments as polled from the transit service, duplication allowed.
pub const MOTIS_SEGMENT: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("motis_segment", "polled_date");

/// One row per scheduled leg, deduped from the polled segments and carrying its geometry.
pub const TRAIN_SEGMENT: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("train_segment", "departure_date");

/// One polled segment, flattened: the trip it belongs to, its resolved agency and train
/// number, its endpoints, its realtime-corrected and scheduled times, and its geometry as
/// the encoded polyline.
///
/// Times are kept as instants and the polyline as the encoded string the service sent,
/// since bronze records what arrived rather than a normalised form of it.
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

/// One scheduled leg, newest capture kept.
///
/// A leg's identity is `(trip_id, from_stop_id, departure)`: `departure` alone is not
/// unique per trip, since minute-resolution timetables let two legs of one trip depart
/// different stops in the same minute.
///
/// The polled segment's encoded polyline is not among these columns: the dataset holds the
/// path decoded, in [`medallion::GEOMETRY`] and [`medallion::PROJECTED_GEOMETRY`], which
/// the writer appends as geometry columns.
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

    /// A train number is stored as the four bytes it is, in both datasets. The row names the
    /// type rather than an integer, which is what keeps a zero — a number no train has — out
    /// of a column that would otherwise accept one.
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
