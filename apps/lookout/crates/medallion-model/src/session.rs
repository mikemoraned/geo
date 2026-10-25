use chrono::{DateTime, NaiveDate, Utc};
use geo_types::Rect;
use medallion::{DatasetSpec, Dated, Geometry, Row, layers};
use serde::{Deserialize, Serialize};

use domain::{Bbox, DeviceId, SessionId, StartedBy};

use crate::GpsReadingRow;

pub const SESSION_V2: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("session_v2", "start_date");

pub const SESSION_SAMPLE_V2: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("session_sample_v2", "sample_date");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRowV2 {
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

impl Row for SessionRowV2 {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = SESSION_V2;
    const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
    const INSTANTS: &'static [&'static str] = &["started_at", "ended_at"];

    fn example() -> Self {
        let device_id = GpsReadingRow::example().device_id;
        Self {
            session_id: SessionId::of(&device_id, DateTime::UNIX_EPOCH),
            device_id,
            started_at: DateTime::UNIX_EPOCH,
            ended_at: DateTime::UNIX_EPOCH,
            sample_count: 1,
            started_by: StartedBy::StartSession,
            gap_seconds: 0,
            lead_seconds: 0,
            bbox: Bbox::of(Rect::new((0.0, 0.0), (1.0, 1.0))),
        }
    }
}

impl Dated for SessionRowV2 {
    fn partition_date(&self) -> NaiveDate {
        self.started_at.date_naive()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSampleRowV2 {
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

impl Row for SessionSampleRowV2 {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = SESSION_SAMPLE_V2;
    const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
    const INSTANTS: &'static [&'static str] = &["t"];

    fn example() -> Self {
        let session = SessionRowV2::example();
        Self {
            session_id: session.session_id,
            device_id: session.device_id,
            t: session.started_at,
            seq: 0,
            lat: 0.0,
            lon: 0.0,
            alt: Some(0.0),
            acc: 0.0,
            speed: Some(0.0),
            heading: Some(0.0),
            implied_speed_mps: Some(0.0),
        }
    }
}

impl Dated for SessionSampleRowV2 {
    fn partition_date(&self) -> NaiveDate {
        self.t.date_naive()
    }
}

#[cfg(test)]
mod tests {
    use arrow::datatypes::DataType;

    use super::*;
    use crate::testing::column;

    #[test]
    fn an_id_is_a_string_column() {
        assert!(matches!(
            column::<SessionRowV2>("session_id").data_type(),
            DataType::Utf8 | DataType::LargeUtf8
        ));
        assert!(matches!(
            column::<SessionSampleRowV2>("session_id").data_type(),
            DataType::Utf8 | DataType::LargeUtf8
        ));
        assert!(matches!(
            column::<SessionRowV2>("device_id").data_type(),
            DataType::Utf8 | DataType::LargeUtf8
        ));
        assert!(matches!(
            column::<SessionSampleRowV2>("device_id").data_type(),
            DataType::Utf8 | DataType::LargeUtf8
        ));
    }

    #[test]
    fn what_started_a_session_is_a_string_column() {
        assert!(matches!(
            column::<SessionRowV2>("started_by").data_type(),
            DataType::Utf8 | DataType::LargeUtf8
        ));
    }

    #[test]
    fn the_bbox_is_a_struct_of_its_four_bounds() {
        let bbox = column::<SessionRowV2>("bbox");
        let DataType::Struct(bounds) = bbox.data_type() else {
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
