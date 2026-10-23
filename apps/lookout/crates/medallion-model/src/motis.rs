use std::fmt::{self, Display};
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, Utc};
use domain::TrainNumber;
use medallion::{DatasetSpec, Dated, Geometry, Row, layers};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumIter, EnumString};

use crate::motis_source::{MotisSourceId, MotisSourceRowV2};

pub const MOTIS_SEGMENT: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("motis_segment", "polled_date");

pub const MOTIS_SEGMENT_V2: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("motis_segment_v2", "polled_date");

pub const TRAIN_SEGMENT_V2: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("train_segment_v2", "departure_date");

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TripId(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a trip id cannot be empty")]
pub struct EmptyTripId;

impl TripId {
    pub fn new(id: impl Into<String>) -> Result<Self, EmptyTripId> {
        let id = id.into();
        if id.is_empty() {
            Err(EmptyTripId)
        } else {
            Ok(Self(id))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for TripId {
    type Err = EmptyTripId;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::new(id)
    }
}

impl Display for TripId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StopId(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a stop id cannot be empty")]
pub struct EmptyStopId;

impl StopId {
    pub fn new(id: impl Into<String>) -> Result<Self, EmptyStopId> {
        let id = id.into();
        if id.is_empty() {
            Err(EmptyStopId)
        } else {
            Ok(Self(id))
        }
    }
}

impl FromStr for StopId {
    type Err = EmptyStopId;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::new(id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgencyId(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("an agency id cannot be empty")]
pub struct EmptyAgencyId;

impl AgencyId {
    pub fn new(id: impl Into<String>) -> Result<Self, EmptyAgencyId> {
        let id = id.into();
        if id.is_empty() {
            Err(EmptyAgencyId)
        } else {
            Ok(Self(id))
        }
    }
}

impl FromStr for AgencyId {
    type Err = EmptyAgencyId;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::new(id)
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    EnumIter,
    EnumString,
    AsRefStr,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum RailMode {
    HighspeedRail,
    LongDistance,
    NightRail,
    RegionalFastRail,
    RegionalRail,
    Rail,
}

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

    fn example() -> Option<Self> {
        Some(Self {
            captured_at: DateTime::UNIX_EPOCH,
            trip_id: "example".to_string(),
            route_name: Some("example".to_string()),
            train_number: TrainNumber::from_gtfs("1"),
            agency_id: Some("example".to_string()),
            agency_name: Some("example".to_string()),
            mode: "RAIL".to_string(),
            route_color: Some("example".to_string()),
            from_stop_id: Some("example".to_string()),
            from_lat: 0.0,
            from_lon: 0.0,
            to_stop_id: Some("example".to_string()),
            to_lat: 0.0,
            to_lon: 0.0,
            departure: DateTime::UNIX_EPOCH,
            arrival: DateTime::UNIX_EPOCH,
            scheduled_departure: DateTime::UNIX_EPOCH,
            scheduled_arrival: DateTime::UNIX_EPOCH,
            realtime: false,
            polyline: "example".to_string(),
        })
    }
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

    fn example() -> Option<Self> {
        Some(Self::of(
            MotisSourceRowV2::example()?.source_id,
            MotisSegmentRow::example()?,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainSegmentRowV2 {
    pub source_id: MotisSourceId,
    pub trip_id: TripId,
    pub route_name: Option<String>,
    pub train_number: Option<TrainNumber>,
    pub agency_id: Option<AgencyId>,
    pub agency_name: Option<String>,
    pub mode: RailMode,
    pub realtime: bool,
    pub from_stop_id: Option<StopId>,
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

    fn example() -> Option<Self> {
        Some(Self {
            source_id: MotisSourceRowV2::example()?.source_id,
            trip_id: TripId("example".to_string()),
            route_name: Some("example".to_string()),
            train_number: TrainNumber::from_gtfs("1"),
            agency_id: Some(AgencyId("example".to_string())),
            agency_name: Some("example".to_string()),
            mode: RailMode::HighspeedRail,
            realtime: false,
            from_stop_id: Some(StopId("example".to_string())),
            departure: DateTime::UNIX_EPOCH,
            arrival: DateTime::UNIX_EPOCH,
        })
    }
}

impl Dated for TrainSegmentRowV2 {
    fn partition_date(&self) -> NaiveDate {
        self.departure.date_naive()
    }
}

#[cfg(test)]
mod tests {
    use arrow::datatypes::DataType;
    use strum::IntoEnumIterator;

    use super::*;
    use crate::testing::{column, column_names};

    #[test]
    fn a_trip_id_is_written_back_as_given() {
        let id: TripId = "20261008_09:12_de-DELFI_123".parse().expect("a trip id");

        assert_eq!(id.to_string(), "20261008_09:12_de-DELFI_123");
        assert_eq!(id.as_str(), "20261008_09:12_de-DELFI_123");
    }

    #[test]
    fn an_empty_trip_id_is_refused() {
        assert_eq!(TripId::new(""), Err(EmptyTripId));
    }

    #[test]
    fn an_empty_stop_or_agency_id_is_refused() {
        assert_eq!(StopId::new(""), Err(EmptyStopId));
        assert_eq!(AgencyId::new(""), Err(EmptyAgencyId));
    }

    #[test]
    fn a_rail_mode_is_named_as_motis_names_it() {
        let names: Vec<String> = RailMode::iter()
            .map(|mode| mode.as_ref().to_string())
            .collect();

        assert_eq!(
            names,
            [
                "HIGHSPEED_RAIL",
                "LONG_DISTANCE",
                "NIGHT_RAIL",
                "REGIONAL_FAST_RAIL",
                "REGIONAL_RAIL",
                "RAIL"
            ]
        );
        for mode in RailMode::iter() {
            assert_eq!(serde_json::to_value(mode).expect("a name"), mode.as_ref());
            assert_eq!(mode.as_ref().parse(), Ok(mode));
        }
    }

    #[test]
    fn a_mode_the_capture_drops_is_no_rail_mode() {
        for name in ["FERRY", "SUBURBAN", "BUS", ""] {
            assert!(name.parse::<RailMode>().is_err(), "{name}");
        }
    }

    #[test]
    fn a_rail_mode_is_stored_as_text() {
        assert_eq!(
            column::<TrainSegmentRowV2>("mode").data_type(),
            &DataType::LargeUtf8
        );
    }

    #[test]
    fn a_capture_carries_every_segment_column_and_its_source() {
        let segment = column_names::<MotisSegmentRow>();
        let capture = column_names::<MotisCaptureRow>();

        assert!(capture.contains("source_id"), "{capture:?}");
        for name in &segment {
            assert!(capture.contains(name), "{name} is missing from {capture:?}");
        }
        assert_eq!(capture.len(), segment.len() + 1);
    }

    #[test]
    fn a_train_number_is_a_four_byte_column_in_both_datasets() {
        for train_number in [
            column::<MotisSegmentRow>("train_number"),
            column::<TrainSegmentRowV2>("train_number"),
        ] {
            assert_eq!(train_number.data_type(), &DataType::UInt32);
        }
    }
}
