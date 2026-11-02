use medallion::{DatasetSpec, Row, layers};
use serde::{Deserialize, Serialize};

use domain::{DeviceId, DeviceType};

pub const RAW_SAMPLE: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("raw_sample", "ingested_date");

pub const GPS_READING_V2: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("gps_reading_v2", "ingested_date");

pub const ACCEL_READING_V2: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("accel_reading_v2", "ingested_date");

pub const DEVICE_SESSION_V2: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("device_session_v2", "ingested_date");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawSampleRow {
    pub md5: String,
    pub received_at: Option<i64>,
    pub json: String,
}

impl Row for RawSampleRow {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = RAW_SAMPLE;
    const INSTANTS: &'static [&'static str] = &["received_at"];

    fn example() -> Self {
        Self {
            md5: "example".to_string(),
            received_at: Some(0),
            json: "{}".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpsReadingRowV2 {
    pub device_id: DeviceId,
    pub t: i64,
    pub lat: f64,
    pub lon: f64,
    pub alt: Option<f64>,
    pub acc: f64,
    pub speed: Option<f64>,
    pub heading: Option<f64>,
}

impl Row for GpsReadingRowV2 {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = GPS_READING_V2;
    const INSTANTS: &'static [&'static str] = &["t"];

    fn example() -> Self {
        Self {
            device_id: DeviceId::new("example").expect("a constant device id is not empty"),
            t: 0,
            lat: 0.0,
            lon: 0.0,
            alt: Some(0.0),
            acc: 0.0,
            speed: Some(0.0),
            heading: Some(0.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccelReadingRowV2 {
    pub device_id: DeviceId,
    pub t: i64,
    pub rms: f64,
    pub peak: f64,
    pub n: u32,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
}

impl Row for AccelReadingRowV2 {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = ACCEL_READING_V2;
    const INSTANTS: &'static [&'static str] = &["t"];

    fn example() -> Self {
        Self {
            device_id: GpsReadingRowV2::example().device_id,
            t: 0,
            rms: 0.0,
            peak: 0.0,
            n: 1,
            x: Some(0.0),
            y: Some(0.0),
            z: Some(0.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSessionRowV2 {
    pub device_id: DeviceId,
    pub t: i64,
    pub device_type: DeviceType,
    pub platform: String,
    pub user_agent: String,
    pub os: Option<String>,
    pub os_version: Option<String>,
}

impl Row for DeviceSessionRowV2 {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = DEVICE_SESSION_V2;
    const INSTANTS: &'static [&'static str] = &["t"];

    fn example() -> Self {
        Self {
            device_id: GpsReadingRowV2::example().device_id,
            t: 0,
            device_type: DeviceType::Iphone,
            platform: "example".to_string(),
            user_agent: "example".to_string(),
            os: Some("example".to_string()),
            os_version: Some("example".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use arrow::datatypes::DataType;

    use super::*;
    use crate::testing::column;

    #[test]
    fn the_class_of_device_is_a_string_column() {
        assert!(matches!(
            column::<DeviceSessionRowV2>("device_type").data_type(),
            DataType::Utf8 | DataType::LargeUtf8
        ));
    }

    #[test]
    fn a_class_is_written_under_the_name_it_was_stored_as() {
        for (class, name) in [
            (DeviceType::Iphone, "iphone"),
            (DeviceType::Ipad, "ipad"),
            (DeviceType::Laptop, "laptop"),
            (DeviceType::Unknown, "unknown"),
        ] {
            assert_eq!(
                serde_json::to_string(&class).expect("write"),
                format!("\"{name}\"")
            );
        }
    }
}
