use chrono::{DateTime, NaiveDate, Utc};
use domain::{Bbox, Percentage, SessionId};
use geo_types::Rect;
use medallion::{DatasetSpec, Dated, Geometry, Projection, Row, layers};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Label, MotisSourceId, MotisSourceRowV4, TrainSegmentRowV4, TripId};

pub const TRAIN_SESSION_V3: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("train_session_v3", "start_date");

pub const TRAIN_SESSION_SAMPLE_V3: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("train_session_sample_v3", "sample_date");

pub fn train_session_id(source_id: &MotisSourceId, trip_id: &TripId) -> SessionId {
    let namespace = Uuid::new_v5(&Uuid::nil(), b"TRAIN_SESSION_NAMESPACE");
    let name = format!("{source_id}\n{trip_id}");
    SessionId::from(Uuid::new_v5(&namespace, name.as_bytes()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeometrySource {
    Stop,
    Interpolated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainSessionRowV3 {
    pub session_id: SessionId,
    pub source_id: MotisSourceId,
    pub source_label: Option<Label>,
    pub trip_id: TripId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub started_at: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub ended_at: DateTime<Utc>,
    pub sample_count: u32,
    pub realtime_percentage: Percentage,
    pub bbox: Bbox,
}

impl Row for TrainSessionRowV3 {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = TRAIN_SESSION_V3;
    const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
    const INSTANTS: &'static [&'static str] = &["started_at", "ended_at"];
    const UNIQUE: &'static [&'static str] = &["session_id"];

    fn example() -> Self {
        let source = MotisSourceRowV4::example();
        let trip_id = TrainSegmentRowV4::example().trip_id;
        Self {
            session_id: train_session_id(&source.source_id, &trip_id),
            source_id: source.source_id,
            source_label: source.label,
            trip_id,
            started_at: DateTime::UNIX_EPOCH,
            ended_at: DateTime::UNIX_EPOCH,
            sample_count: 1,
            realtime_percentage: Percentage::new(100.0).expect("100 is a percentage"),
            bbox: Bbox::of(Rect::new((0.0, 0.0), (1.0, 1.0))),
        }
    }
}

impl Dated for TrainSessionRowV3 {
    fn partition_date(&self) -> NaiveDate {
        self.started_at.date_naive()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainSessionSampleRowV3 {
    pub session_id: SessionId,
    pub source_id: MotisSourceId,
    pub trip_id: TripId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub t: DateTime<Utc>,
    pub seq: u32,
    pub lat: f64,
    pub lon: f64,
    pub implied_speed_mps: Option<f64>,
    pub geometry_source: GeometrySource,
    pub realtime: bool,
}

impl Row for TrainSessionSampleRowV3 {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = TRAIN_SESSION_SAMPLE_V3;
    const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
    const INSTANTS: &'static [&'static str] = &["t"];

    fn example() -> Self {
        let session = TrainSessionRowV3::example();
        Self {
            session_id: session.session_id,
            source_id: session.source_id,
            trip_id: session.trip_id,
            t: session.started_at,
            seq: 0,
            lat: 0.0,
            lon: 0.0,
            implied_speed_mps: Some(0.0),
            geometry_source: GeometrySource::Stop,
            realtime: true,
        }
    }
}

impl Dated for TrainSessionSampleRowV3 {
    fn partition_date(&self) -> NaiveDate {
        self.t.date_naive()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainSessionSamplePositionV3 {
    pub session_id: SessionId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub t: DateTime<Utc>,
    pub seq: u32,
    pub lat: f64,
    pub lon: f64,
}

impl Projection for TrainSessionSamplePositionV3 {
    type Of = TrainSessionSampleRowV3;

    fn example() -> Self {
        let sample = TrainSessionSampleRowV3::example();
        Self {
            session_id: sample.session_id,
            t: sample.t,
            seq: sample.seq,
            lat: sample.lat,
            lon: sample.lon,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use arrow::datatypes::{DataType, Field, TimeUnit};

    use super::*;
    use crate::testing::{column, column_names};
    use crate::{SessionRowV3, SessionSampleRowV3};

    fn source(id: &str) -> MotisSourceId {
        serde_json::from_value(serde_json::json!(id)).expect("a source id")
    }

    fn trip(id: &str) -> TripId {
        TripId::new(id).expect("a trip id")
    }

    #[test]
    fn a_sample_position_reads_only_columns_its_sample_holds() {
        let held = column_names::<TrainSessionSampleRowV3>();
        let read: BTreeSet<String> = medallion::projected_columns::<TrainSessionSamplePositionV3>()
            .expect("describe the position")
            .into_iter()
            .collect();

        assert!(
            read.is_subset(&held),
            "the position reads {read:?}, and the sample holds only {held:?}"
        );
    }

    fn swapped<T: Row>(left_out: &[&str], added: &[&str]) -> BTreeSet<String> {
        column_names::<T>()
            .into_iter()
            .filter(|name| !left_out.contains(&name.as_str()))
            .chain(added.iter().map(|name| name.to_string()))
            .collect()
    }

    #[test]
    fn an_id_is_the_same_for_the_same_source_and_trip() {
        assert_eq!(
            train_session_id(&source("source-a"), &trip("trip-a")),
            train_session_id(&source("source-a"), &trip("trip-a"))
        );
    }

    #[test]
    fn an_id_never_changes_once_written() {
        assert_eq!(
            train_session_id(&source("source-a"), &trip("trip-a")).to_string(),
            "8fe37996-52d2-5c19-b91d-dd62f0980498"
        );
    }

    #[test]
    fn an_id_differs_by_source_and_by_trip() {
        let id = train_session_id(&source("source-a"), &trip("trip-a"));

        assert_ne!(id, train_session_id(&source("source-b"), &trip("trip-a")));
        assert_ne!(id, train_session_id(&source("source-a"), &trip("trip-b")));
    }

    #[test]
    fn an_id_cannot_be_mistaken_for_a_recorded_sessions() {
        let device = domain::DeviceId::new("source-a").expect("a device id");
        let started_at = DateTime::from_timestamp_millis(0).expect("an instant");

        assert_ne!(
            train_session_id(&source("source-a"), &trip("0")),
            SessionId::of(&device, started_at)
        );
    }

    #[test]
    fn a_derived_id_could_name_a_partition() {
        let id = train_session_id(&source("source-a"), &trip("20261008_09:12_de-DELFI_123"));

        assert_eq!(SessionId::new(id.to_string()).expect("a valid id"), id);
    }

    #[test]
    fn where_a_samples_geometry_came_from_is_stored_as_its_name() {
        assert!(matches!(
            column::<TrainSessionSampleRowV3>("geometry_source").data_type(),
            DataType::Utf8 | DataType::LargeUtf8
        ));
        assert_eq!(
            serde_json::to_value([GeometrySource::Stop, GeometrySource::Interpolated])
                .expect("serialise"),
            serde_json::json!(["stop", "interpolated"])
        );
    }

    #[test]
    fn a_train_session_has_a_recorded_sessions_columns_but_what_only_a_device_reports() {
        assert_eq!(
            column_names::<TrainSessionRowV3>(),
            swapped::<SessionRowV3>(
                &["device_id", "started_by", "gap_seconds", "lead_seconds"],
                &[
                    "source_id",
                    "source_label",
                    "trip_id",
                    "realtime_percentage"
                ]
            )
        );
    }

    #[test]
    fn a_train_session_keeps_the_columns_it_is_stored_with() {
        let instant = DataType::Timestamp(TimeUnit::Millisecond, Some("UTC".into()));
        let bound = |name| Field::new(name, DataType::Float64, false);

        assert_eq!(
            medallion::fields::<TrainSessionRowV3>().expect("describe the rows"),
            [
                Field::new("session_id", DataType::LargeUtf8, false),
                Field::new("source_id", DataType::LargeUtf8, false),
                Field::new("source_label", DataType::LargeUtf8, true),
                Field::new("trip_id", DataType::LargeUtf8, false),
                Field::new("started_at", instant.clone(), true),
                Field::new("ended_at", instant, true),
                Field::new("sample_count", DataType::UInt32, false),
                Field::new("realtime_percentage", DataType::Float64, false),
                Field::new(
                    "bbox",
                    DataType::Struct(
                        ["xmin", "ymin", "xmax", "ymax"]
                            .map(bound)
                            .into_iter()
                            .collect()
                    ),
                    false
                ),
            ]
            .map(Arc::new)
        );
    }

    #[test]
    fn a_source_without_a_label_is_stored_as_null() {
        let field = column::<TrainSessionRowV3>("source_label");

        assert!(
            matches!(field.data_type(), DataType::Utf8 | DataType::LargeUtf8),
            "{field:?}"
        );
        assert!(field.is_nullable(), "{field:?}");
    }

    #[test]
    fn a_train_sample_has_a_recorded_samples_columns_but_what_only_a_device_reports() {
        assert_eq!(
            column_names::<TrainSessionSampleRowV3>(),
            swapped::<SessionSampleRowV3>(
                &["device_id", "alt", "acc", "speed", "heading"],
                &["source_id", "trip_id", "geometry_source", "realtime"]
            )
        );
    }
}
