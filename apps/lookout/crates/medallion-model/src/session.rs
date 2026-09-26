use chrono::{DateTime, NaiveDate, Utc};
use medallion::{DatasetSpec, Dated, Geometry, Row, layers};
use serde::{Deserialize, Serialize};

use domain::{Bbox, DeviceId, SessionId, StartedBy};

pub const SESSION: DatasetSpec<layers::Silver> = DatasetSpec::partitioned("session", "start_date");

pub const SESSION_SAMPLE: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("session_sample", "sample_date");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRow {
    pub session_id: SessionId,
    pub device_id: DeviceId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub started_at: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub ended_at: DateTime<Utc>,
    pub sample_count: u32,
    pub started_by: StartedBy,
    pub gap_seconds: u32,
    pub lead_seconds: u32,
    pub bbox: Bbox,
}

impl Row for SessionRow {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = SESSION;
    const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
    const INSTANTS: &'static [&'static str] = &["started_at", "ended_at"];
}

impl Dated for SessionRow {
    fn partition_date(&self) -> NaiveDate {
        self.started_at.date_naive()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSampleRow {
    pub session_id: SessionId,
    pub device_id: DeviceId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub t: DateTime<Utc>,
    pub seq: u32,
    pub lat: f64,
    pub lon: f64,
    pub alt: Option<f64>,
    pub acc: f64,
    pub speed: Option<f64>,
    pub heading: Option<f64>,
    pub implied_speed_mps: Option<f64>,
}

impl Row for SessionSampleRow {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = SESSION_SAMPLE;
    const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
    const INSTANTS: &'static [&'static str] = &["t"];
}

impl Dated for SessionSampleRow {
    fn partition_date(&self) -> NaiveDate {
        self.t.date_naive()
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
    fn an_id_is_a_string_column() {
        assert!(matches!(
            column::<SessionRow>("session_id"),
            DataType::Utf8 | DataType::LargeUtf8
        ));
        assert!(matches!(
            column::<SessionSampleRow>("session_id"),
            DataType::Utf8 | DataType::LargeUtf8
        ));
        assert!(matches!(
            column::<SessionRow>("device_id"),
            DataType::Utf8 | DataType::LargeUtf8
        ));
        assert!(matches!(
            column::<SessionSampleRow>("device_id"),
            DataType::Utf8 | DataType::LargeUtf8
        ));
    }

    #[test]
    fn what_started_a_session_is_a_string_column() {
        assert!(matches!(
            column::<SessionRow>("started_by"),
            DataType::Utf8 | DataType::LargeUtf8
        ));
    }

    #[test]
    fn the_envelope_is_a_struct_of_its_four_bounds() {
        let DataType::Struct(bounds) = column::<SessionRow>("bbox") else {
            panic!("bbox should be a struct");
        };

        assert_eq!(
            bounds
                .iter()
                .map(|bound| bound.name().as_str())
                .collect::<Vec<_>>(),
            ["xmin", "ymin", "xmax", "ymax"]
        );
    }

    #[test]
    fn an_id_is_the_same_for_the_same_device_and_start() {
        let device = DeviceId::new("device-a").expect("device id");
        let started_at = DateTime::from_timestamp_millis(1_700_000_000_000).expect("instant");

        assert_eq!(
            SessionId::of(&device, started_at),
            SessionId::of(&device, started_at)
        );
    }

    #[test]
    fn an_id_differs_by_device_and_by_start() {
        let a = DeviceId::new("device-a").expect("device id");
        let b = DeviceId::new("device-b").expect("device id");
        let started_at = DateTime::from_timestamp_millis(1_700_000_000_000).expect("instant");
        let later = started_at + chrono::Duration::milliseconds(1);

        assert_ne!(SessionId::of(&a, started_at), SessionId::of(&b, started_at));
        assert_ne!(SessionId::of(&a, started_at), SessionId::of(&a, later));
    }

    #[test]
    fn a_derived_id_could_name_a_partition() {
        let id = SessionId::of(&DeviceId::new("device-a").expect("device id"), Utc::now());

        assert_eq!(SessionId::new(id.to_string()).expect("valid id"), id);
    }

    #[test]
    fn an_id_that_could_not_name_a_partition_is_rejected() {
        assert!(SessionId::new("a/b").is_err());
        assert!(SessionId::new("").is_err());
        assert_eq!(
            "0192f0c3d4e5".parse::<SessionId>().unwrap().to_string(),
            "0192f0c3d4e5"
        );
    }
}
