//! Sessions: one contiguous run of GPS fixes from one device, and the fixes within it.
//!
//! The two are separate datasets because they are partitioned by different dates. A fix is
//! partitioned by when it was recorded and a session by when it started, so a session
//! crossing midnight has its fixes split over two partitions and is reassembled by
//! `session_id` — which is therefore carried on every fix, along with `device_id`, so a
//! partition of fixes is readable without joining back to the sessions.
//!
//! Both datasets **keep every fix and flag the doubtful ones** rather than filtering. What
//! counts as a usable fix is a threshold of whoever is reading — a ground truth and a
//! predictor are entitled to disagree about it — so the columns a filter needs are carried
//! and the line is drawn by the consumer, not baked into the store.

use std::fmt::{self, Display};
use std::str::FromStr;

use chrono::{DateTime, Utc};
use medallion::{DatasetSpec, Layer, PartitionValue, PathError, Row};
use serde::{Deserialize, Serialize};

/// One contiguous run of fixes from one device.
pub const SESSION: DatasetSpec = DatasetSpec::partitioned(Layer::Silver, "session", "start_date");

/// The fixes making up the sessions, one row per deduped bronze reading.
pub const SESSION_FIX: DatasetSpec =
    DatasetSpec::partitioned(Layer::Silver, "session_fix", "fix_date");

/// Identifies one session, on the session and on each of its fixes.
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
    /// The device announced a new session.
    StartSession,
    /// The interval since the previous fix exceeded the threshold.
    Gap,
    /// The first fix recorded for this device, with nothing before it. Sessions begin this
    /// way where the device sent no session announcement at all.
    FirstSeen,
}

/// The envelope of a session's fixes, in the same axis names the upstream reference data
/// uses for its own envelopes.
///
/// It is stored rather than derived on read because "which sessions could have come near
/// this place" is the question sessions are searched by, and answering it should not
/// require opening the fixes.
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
    pub device_id: String,
    /// The first fix in the session.
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub started_at: DateTime<Utc>,
    /// The last fix in the session. A session is only closed in the sense that no later
    /// fix has been recorded yet: the most recent one grows as more arrive.
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub ended_at: DateTime<Utc>,
    pub fix_count: u32,
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

/// One fix within a session: the reading as the device reported it, plus what a reader
/// needs to judge whether to trust it.
///
/// A fix is identified by `(device_id, t)`, the identity it is deduped from bronze on. Its
/// position is held in [`medallion::GEOMETRY`] and [`medallion::PROJECTED_GEOMETRY`] as a
/// Point, which the writer appends as geometry columns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionFixRow {
    pub session_id: SessionId,
    pub device_id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub t: DateTime<Utc>,
    /// Where the fix falls in its session, counting from zero.
    pub seq: u32,
    pub lat: f64,
    pub lon: f64,
    pub alt: Option<f64>,
    /// Accuracy as the device reported it, in metres.
    pub acc: f64,
    pub speed: Option<f64>,
    pub heading: Option<f64>,
    /// The speed the step from the previous fix in the session implies, in metres per
    /// second — absent for the first fix, which has no previous one. A value far above
    /// what the vehicle could do is the mark of a bad fix, so this is carried rather than
    /// used here to discard one.
    pub implied_speed_mps: Option<f64>,
    /// Whether this fix is timed before the fix preceding it in the session.
    pub backwards_in_time: bool,
}

impl Row for SessionFixRow {
    const DATASET: DatasetSpec = SESSION_FIX;
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
    /// to be — a reader joining fixes to sessions compares ids as values.
    #[test]
    fn an_id_is_a_string_column() {
        assert!(matches!(
            column::<SessionRow>("session_id"),
            DataType::Utf8 | DataType::LargeUtf8
        ));
        assert!(matches!(
            column::<SessionFixRow>("session_id"),
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
