//! Water crossings: the places a train can see water, and the ones a session passed.
//!
//! The two are partitioned differently because they are different kinds of thing.
//! [`WATER_CROSSING`] is derived from reference data and changes only when an extract or the
//! collapse that builds it does, so it is laid out by `country=` — the level its projected
//! geometry's zone is chosen at, and the level a reader restricts by.
//! [`SESSION_CROSSING`] is an observation: it is laid out by the date it happened, the same
//! key `session_sample` uses for the sample it is derived from, so the same key means the same
//! thing across the datasets a reader joins.
//!
//! **The collapse is the definition of a crossing, so its tuning is stored on the row.** This
//! is deliberately unlike the sessions, which keep every sample and let each consumer draw its
//! own line: there the thresholds belong to whoever is reading, whereas here two runs that
//! collapsed differently do not agree on what a crossing *is*, and a ground truth and a
//! prediction that count different things cannot be compared. Retuning is therefore a rebuild,
//! and the columns say which tuning a row was built under.

use chrono::{DateTime, NaiveDate, Utc};
use domain::{CrossingCompactId, CrossingId, DeviceId, SessionId};
use medallion::{COUNTRY, DatasetSpec, Dated, Geometry, Row, layers};
use serde::{Deserialize, Serialize};

/// One place a stretch of track meets one body of water.
pub const WATER_CROSSING: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("water_crossing", COUNTRY);

/// The ground truth: a crossing having been passed in a session.
pub const SESSION_CROSSING: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("session_crossing", "crossed_date");

/// How a stretch of track and a body of water meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlapKind {
    /// The track runs along or across an area of water, so the overlap has a length.
    Line,
    /// The track crosses the centreline of a watercourse, which has no width to overlap.
    Point,
}

/// One crossing: which water, which stretch of track, and where along it they meet.
///
/// A crossing is the collapsed representative of the parts a stretch of track and one body of
/// water overlap in — a bridge is one crossing, not one per span — so the row carries both its
/// own overlap and the total over the parts merged into it.
///
/// Its position is held in [`medallion::GEOMETRY`] and [`medallion::PROJECTED_GEOMETRY`] as a
/// Point, which the writer appends as geometry columns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaterCrossingRow {
    pub crossing_id: CrossingId,
    /// The crossing named in four bytes, for a device with no room for `crossing_id`.
    ///
    /// Derived from `crossing_id` and carried here rather than worked out by whoever packs a
    /// buffer, so there is one answer to what a crossing is called on a device, and it is
    /// checked for uniqueness where every other property of the dataset is.
    pub crossing_compact_id: CrossingCompactId,
    /// The water body, by its id in the upstream reference data.
    pub water_id: String,
    pub water_subtype: Option<String>,
    pub water_class: Option<String>,
    /// The connected stretch of physical track, named canonically from its own members
    /// rather than by a label a run happened to assign.
    pub track_id: String,
    /// The upstream segment the representative part lies on. One of possibly several making
    /// up `track_id`.
    pub rail_id: String,
    pub rail_class: Option<String>,
    pub overlap_kind: OverlapKind,
    /// The length of the representative part's overlap, in metres. Zero for a point overlap,
    /// which has no length.
    pub overlap_m: f64,
    /// `overlap_m` summed over every part merged into this crossing.
    pub total_overlap_m: f64,
    pub merged_parts: u32,
    /// Where along `rail_id` the crossing sits, from 0 at its start to 1 at its end.
    pub frac: f64,
    /// The extraction the upstream rows came from, so a crossing can be traced to a release.
    pub extract_id: String,
    /// How close two parts had to be for the run that derived this row to merge them, in
    /// metres, so a crossing built under one tuning is still interpretable after it changes.
    pub merge_distance_m: f64,
    /// The shortest overlap that run kept, in metres. Interpretable for the same reason.
    pub min_crossing_m: f64,
}

impl Row for WaterCrossingRow {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = WATER_CROSSING;
    const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
    /// Both names of a crossing identify it on their own: the store's, and the four bytes a
    /// device holds instead. A compact id shared by two crossings is one a device could not tell
    /// apart, which is why it is refused here rather than at the buffer.
    const UNIQUE: &'static [&'static str] = &["crossing_id", "crossing_compact_id"];
}

/// One crossing passed in one session: when, and on what evidence.
///
/// The instant is that of the session's nearest sample to the crossing, which is as precise as
/// the recording gets — nothing observes the passing itself. `distance_m` and `samples_within`
/// are what say how good that evidence is: a crossing matched by one distant sample and one
/// matched by twenty close ones are both recorded, and a reader weighs them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionCrossingRow {
    pub session_id: SessionId,
    pub crossing_id: CrossingId,
    /// Carried so a partition is readable without joining back to the sessions, as
    /// `session_sample` carries it for the same reason.
    pub device_id: DeviceId,
    /// The instant of the nearest sample.
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub crossed_at: DateTime<Utc>,
    /// How far that sample was from the crossing, in metres.
    pub distance_m: f64,
    /// How many of the session's samples fell within the match radius.
    pub samples_within: u32,
    /// The radius the run that derived this row matched within, in metres, so a match made
    /// under one radius is still interpretable after it changes.
    pub match_radius_m: f64,
}

impl Row for SessionCrossingRow {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = SESSION_CROSSING;
    const INSTANTS: &'static [&'static str] = &["crossed_at"];
}

impl Dated for SessionCrossingRow {
    fn partition_date(&self) -> NaiveDate {
        self.crossed_at.date_naive()
    }
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

    /// Both names of a crossing are stored as the values they are, rather than as whatever a
    /// newtype around them might become: the long one as text, the compact one as four bytes.
    #[test]
    fn a_compact_id_is_a_four_byte_column() {
        assert_eq!(
            column::<WaterCrossingRow>("crossing_compact_id"),
            DataType::UInt32
        );
    }

    /// The id joining the two datasets is the same kind of value in both, so a reader joins
    /// them by comparing values rather than by converting one side.
    #[test]
    fn a_crossing_id_is_a_string_column_in_both_datasets() {
        assert!(matches!(
            column::<WaterCrossingRow>("crossing_id"),
            DataType::Utf8 | DataType::LargeUtf8
        ));
        assert!(matches!(
            column::<SessionCrossingRow>("crossing_id"),
            DataType::Utf8 | DataType::LargeUtf8
        ));
    }

    /// How the track and the water meet is one of a closed set of names, and is stored as
    /// that name.
    #[test]
    fn the_kind_of_overlap_is_a_string_column() {
        assert!(matches!(
            column::<WaterCrossingRow>("overlap_kind"),
            DataType::Utf8 | DataType::LargeUtf8
        ));
    }

    /// An id is a candidate key for a partition wherever a reader lays one out by it. What
    /// makes an id writable is the id's own rule; what is asserted here is that the store
    /// depends on it, so a change to either side fails rather than waiting for a path.
    #[test]
    fn any_id_can_name_a_partition() {
        let id: CrossingId = "08b2a5c1fffffff-08f2a5c1".parse().expect("a name");

        assert!(medallion::PartitionValue::new(id.to_string()).is_ok());
    }

    /// The tuning a row was built under travels with the row, since two rows collapsed
    /// differently are not describing the same thing.
    #[test]
    fn a_crossing_carries_the_tuning_that_produced_it() {
        for tuning in ["merge_distance_m", "min_crossing_m"] {
            assert_eq!(column::<WaterCrossingRow>(tuning), DataType::Float64);
        }
        assert_eq!(
            column::<SessionCrossingRow>("match_radius_m"),
            DataType::Float64
        );
    }
}
