use medallion::{DatasetSpec, Row, layers};
use serde::{Deserialize, Serialize};

use domain::{DeviceId, DeviceType};

pub const RAW_SAMPLE: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("raw_sample", "ingested_date");

pub const GPS_READING: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("gps_reading", "ingested_date");

pub const ACCEL_READING: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("accel_reading", "ingested_date");

pub const DEVICE_SESSION: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("device_session", "ingested_date");

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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpsReadingRow {
    pub device_id: DeviceId,
    pub t: i64,
    pub lat: f64,
    pub lon: f64,
    pub alt: Option<f64>,
    pub acc: f64,
    pub speed: Option<f64>,
    pub heading: Option<f64>,
}

impl Row for GpsReadingRow {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = GPS_READING;
    const INSTANTS: &'static [&'static str] = &["t"];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccelReadingRow {
    pub device_id: DeviceId,
    pub t: i64,
    pub rms: f64,
    pub peak: f64,
    pub n: u32,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
}

impl Row for AccelReadingRow {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = ACCEL_READING;
    const INSTANTS: &'static [&'static str] = &["t"];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSessionRow {
    pub device_id: DeviceId,
    pub t: i64,
    pub device_type: DeviceType,
    pub platform: String,
    pub user_agent: String,
    pub os: Option<String>,
    pub os_version: Option<String>,
}

impl Row for DeviceSessionRow {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = DEVICE_SESSION;
    const INSTANTS: &'static [&'static str] = &["t"];
}

#[cfg(test)]
mod tests {
    use arrow::datatypes::DataType;

    use super::*;

    #[test]
    fn the_class_of_device_is_a_string_column() {
        let fields = medallion::fields::<DeviceSessionRow>().expect("describe the rows");
        let column = fields
            .iter()
            .find(|field| field.name() == "device_type")
            .expect("a device_type column");

        assert!(matches!(
            column.data_type(),
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
