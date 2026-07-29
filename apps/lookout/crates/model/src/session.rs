//! Sessions: one contiguous run of GPS samples from one device, and the samples within it.
//!
//! Both live under a `country=` partition, since the CRS of a projected geometry column is
//! stated once per file and the zone is chosen per country. Below that they are separate
//! datasets because they are partitioned by different dates: a sample by when it was
//! recorded and a session by when it started, so a session
//! crossing midnight has its samples split over two partitions and is reassembled by
//! `session_id` — which is therefore carried on every sample, along with `device_id`, so a
//! partition of samples is readable without joining back to the sessions.
//!
//! Both datasets **keep every sample and flag the doubtful ones** rather than filtering. What
//! counts as a usable sample is a threshold of whoever is reading — a ground truth and a
//! predictor are entitled to disagree about it — so the columns a filter needs are carried
//! and the line is drawn by the consumer, not baked into the store.

use std::fmt::{self, Display};
use std::str::FromStr;

use chrono::{DateTime, Utc};
use medallion::{DatasetSpec, Layer, PartitionValue, PathError, Row};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::device::DeviceId;

/// One contiguous run of samples from one device.
pub const SESSION: DatasetSpec = DatasetSpec::partitioned(Layer::Silver, "session", "start_date");

/// The samples making up the sessions, one row per deduped bronze reading.
pub const SESSION_SAMPLE: DatasetSpec =
    DatasetSpec::partitioned(Layer::Silver, "session_sample", "sample_date");

/// The namespace session ids are minted in, so an id derived here cannot collide with a
/// name-based id derived from the same values for anything else.
const SESSION_NAMESPACE: Uuid = Uuid::from_u128(0x8f9c_1d3a_6b47_4e21_9a05_c7d8_e2f4_1b60);

/// Identifies one session, on the session and on each of its samples.
///
/// Derived from what the session *is* rather than minted per run, so a run that re-derives
/// a session it has already written lands on the same id and rewrites it in place.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    /// An existing id, rejecting anything that could not also name a partition — an id is
    /// a candidate key for a partition wherever a reader chooses to lay one out by it.
    pub fn new(id: impl Into<String>) -> Result<Self, PathError> {
        let id = id.into();
        PartitionValue::new(id.clone())?;
        Ok(Self(id))
    }

    /// The id of the session `device` began at `started_at`.
    ///
    /// A name-based UUID over exactly what identifies the session, so any run — or any
    /// reader wanting to name a session it has only the boundaries of — arrives at the
    /// same id without consulting what has already been written.
    pub fn of(device: &DeviceId, started_at: DateTime<Utc>) -> Self {
        let name = format!("{device}/{}", started_at.timestamp_millis());
        Self(Uuid::new_v5(&SESSION_NAMESPACE, name.as_bytes()).to_string())
    }
}

impl FromStr for SessionId {
    type Err = PathError;

    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Self::new(id)
    }
}

impl Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// What ended the previous session and so began this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartedBy {
    /// The device reported the start of a session.
    StartSession,
    /// The interval since the previous sample exceeded the threshold.
    Gap,
    /// The first sample recorded for this device, with nothing before it. Sessions begin this
    /// way where the device reported no session start at all.
    FirstSeen,
}

/// The envelope of a session's samples, in the same axis names the upstream reference data
/// uses for its own envelopes.
///
/// It is stored rather than derived on read because "which sessions could have come near
/// this place" is the question sessions are searched by, and answering it should not
/// require opening the samples.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bbox {
    pub xmin: f64,
    pub ymin: f64,
    pub xmax: f64,
    pub ymax: f64,
}

/// One session: which device recorded it, when it ran, and the path it took.
///
/// The path is held in [`medallion::GEOMETRY`] and [`medallion::PROJECTED_GEOMETRY`] as a
/// LineString, which the writer appends as geometry columns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRow {
    pub session_id: SessionId,
    pub device_id: DeviceId,
    /// The first sample in the session.
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub started_at: DateTime<Utc>,
    /// The last sample in the session. A session is only closed in the sense that no later
    /// sample has been recorded yet: the most recent one grows as more arrive.
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub ended_at: DateTime<Utc>,
    pub sample_count: u32,
    pub started_by: StartedBy,
    /// The gap threshold the run that derived this session applied, so a session derived
    /// under one threshold is still interpretable once the default changes.
    pub gap_seconds: u32,
    pub bbox: Bbox,
}

impl Row for SessionRow {
    const DATASET: DatasetSpec = SESSION;
    const INSTANTS: &'static [&'static str] = &["started_at", "ended_at"];
}

/// One sample within a session: the reading as the device reported it, plus what a reader
/// needs to judge whether to trust it.
///
/// A sample is identified by `(device_id, t)`, the identity it is deduped from bronze on. Its
/// position is held in [`medallion::GEOMETRY`] and [`medallion::PROJECTED_GEOMETRY`] as a
/// Point, which the writer appends as geometry columns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSampleRow {
    pub session_id: SessionId,
    pub device_id: DeviceId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub t: DateTime<Utc>,
    /// Where the sample falls in its session, counting from zero.
    pub seq: u32,
    pub lat: f64,
    pub lon: f64,
    pub alt: Option<f64>,
    /// Accuracy as the device reported it, in metres.
    pub acc: f64,
    pub speed: Option<f64>,
    pub heading: Option<f64>,
    /// The speed the step from the previous sample in the session implies, in metres per
    /// second — absent for the first sample, which has no previous one. A value far above
    /// what the vehicle could do is the mark of a bad sample, so this is carried rather than
    /// used here to discard one.
    pub implied_speed_mps: Option<f64>,
}

impl Row for SessionSampleRow {
    const DATASET: DatasetSpec = SESSION_SAMPLE;
    const INSTANTS: &'static [&'static str] = &["t"];
}

#[cfg(test)]
mod tests {
    use arrow::datatypes::DataType;

    use super::*;

    /// The column named `name` of `T`'s schema.
    fn column<T: Row>(name: &str) -> DataType {
        medallion::fields::<T>()
            .expect("describe the rows")
            .iter()
            .find(|field| field.name() == name)
            .unwrap_or_else(|| panic!("no column named `{name}`"))
            .data_type()
            .clone()
    }

    /// An id is stored as the string it reads as, not as the shape its Rust type happens
    /// to be — a reader joining samples to sessions compares ids as values.
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

    /// How a session began is one of a closed set of names, and is stored as that name.
    #[test]
    fn what_started_a_session_is_a_string_column() {
        assert!(matches!(
            column::<SessionRow>("started_by"),
            DataType::Utf8 | DataType::LargeUtf8
        ));
    }

    /// The envelope is one struct column of four bounds, so a predicate on it names the
    /// axis it means rather than an offset into something.
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

    /// A session is identified by the device and the instant it began, so the same session
    /// re-derived from grown bronze keeps the id already written against it.
    #[test]
    fn an_id_is_the_same_for_the_same_device_and_start() {
        let device = DeviceId::new("device-a").expect("device id");
        let started_at = DateTime::from_timestamp_millis(1_700_000_000_000).expect("instant");

        assert_eq!(
            SessionId::of(&device, started_at),
            SessionId::of(&device, started_at)
        );
    }

    /// Two sessions that differ in either part of that identity are different sessions.
    #[test]
    fn an_id_differs_by_device_and_by_start() {
        let a = DeviceId::new("device-a").expect("device id");
        let b = DeviceId::new("device-b").expect("device id");
        let started_at = DateTime::from_timestamp_millis(1_700_000_000_000).expect("instant");
        let later = started_at + chrono::Duration::milliseconds(1);

        assert_ne!(SessionId::of(&a, started_at), SessionId::of(&b, started_at));
        assert_ne!(SessionId::of(&a, started_at), SessionId::of(&a, later));
    }

    /// A derived id has to survive the same partition-naming rule an existing one is
    /// checked against, since both name the same things.
    #[test]
    fn a_derived_id_could_name_a_partition() {
        let id = SessionId::of(&DeviceId::new("device-a").expect("device id"), Utc::now());

        assert_eq!(SessionId::new(id.to_string()).expect("valid id"), id);
    }

    /// An id names a partition wherever a reader lays one out by it, so one that could not
    /// is rejected at construction rather than when a path is built from it.
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
