use chrono::{DateTime, NaiveDate, Utc};
use domain::{Bbox, SessionId};
use geo_types::Rect;
use medallion::{DatasetSpec, Dated, Geometry, Row, layers};
use serde::{Deserialize, Serialize};

use crate::{
    GeometrySource, Label, MotisSourceId, MotisSourceRowV4, TrainSegmentRowV4, TripId,
    train_session_id,
};

pub const TRAIN_SESSION_V2: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("train_session_v2", "start_date");

pub const TRAIN_SESSION_SAMPLE_V2: DatasetSpec<layers::Silver> =
    DatasetSpec::partitioned("train_session_sample_v2", "sample_date");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainSessionRowV2 {
    pub session_id: SessionId,
    pub source_id: MotisSourceId,
    pub source_label: Option<Label>,
    pub trip_id: TripId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub started_at: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub ended_at: DateTime<Utc>,
    pub sample_count: u32,
    pub bbox: Bbox,
}

impl Row for TrainSessionRowV2 {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = TRAIN_SESSION_V2;
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
            bbox: Bbox::of(Rect::new((0.0, 0.0), (1.0, 1.0))),
        }
    }
}

impl Dated for TrainSessionRowV2 {
    fn partition_date(&self) -> NaiveDate {
        self.started_at.date_naive()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainSessionSampleRowV2 {
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
}

impl Row for TrainSessionSampleRowV2 {
    type Layer = layers::Silver;
    const DATASET: DatasetSpec<Self::Layer> = TRAIN_SESSION_SAMPLE_V2;
    const GEOMETRY: Geometry = Geometry::LatLonAndProjected;
    const INSTANTS: &'static [&'static str] = &["t"];

    fn example() -> Self {
        let session = TrainSessionRowV2::example();
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
        }
    }
}

impl Dated for TrainSessionSampleRowV2 {
    fn partition_date(&self) -> NaiveDate {
        self.t.date_naive()
    }
}
