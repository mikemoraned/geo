//! The bronze telemetry datasets: what a device sent, and what was read out of it.
//!
//! Every payload lands verbatim in [`RAW_SAMPLE`], and the readings interpreted from it in
//! one dataset per sensor. Sensors are split rather than sharing one under a `sensor=`
//! partition because they carry different columns, and a dataset is one schema.

use medallion::{layers, DatasetSpec, Row};
use serde::{Deserialize, Serialize};

use crate::device::DeviceId;

/// Every payload the telemetry queue carried, verbatim. The lossless record the other
/// telemetry datasets are interpreted from.
pub const RAW_SAMPLE: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("raw_sample", "ingested_date");

/// GPS samples interpreted from the payloads.
pub const GPS_READING: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("gps_reading", "ingested_date");

/// Accelerometer readings interpreted from the payloads.
pub const ACCEL_READING: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("accel_reading", "ingested_date");

/// The metadata a device announces when it starts a session.
pub const DEVICE_SESSION: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("device_session", "ingested_date");

/// One archived payload, exactly as it arrived.
///
/// `received_at` is optional because a payload restored from an older archive may predate
/// receipt times being recorded at all; a payload off the queue always carries one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawSampleRow {
    /// Identifies the payload, so re-ingesting the same one is recognisable downstream.
    pub md5: String,
    /// When the server stamped it on receipt, where that was recorded.
    pub received_at: Option<i64>,
    pub json: String,
}

impl Row for RawSampleRow {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = RAW_SAMPLE;
    const INSTANTS: &'static [&'static str] = &["received_at"];
}

/// One GPS sample as the device reported it.
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

/// One accelerometer reading: the aggregates over the window it covers, and the last raw
/// sample within it.
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

/// One session start: which device began recording, when, and what it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSessionRow {
    pub device_id: DeviceId,
    pub t: i64,
    pub device_type: String,
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
