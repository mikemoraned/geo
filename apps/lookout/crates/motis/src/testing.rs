#![cfg(test)]

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use geo_types::Point;
use medallion::{AppendError, Countries, Country, Root};
use medallion_model::{Feed, MotisCaptureRow, MotisSource, MotisSourceId, TripId};
use url::Url;

use crate::api::types::{Mode, TripSegment};
use crate::client::TripDetails;
use crate::landing::segment_row;
use crate::segment::SingleTripSegment;

pub(crate) const TRIPS: &str = include_str!("../tests/fixtures/trips.json");

pub(crate) struct CaptureLog {
    root: Root,
    source_id: MotisSourceId,
}

impl CaptureLog {
    pub(crate) fn new(root: Root, source_id: MotisSourceId) -> Self {
        Self { root, source_id }
    }

    pub(crate) async fn append(
        &self,
        captured_at: DateTime<Utc>,
        segments: &[SingleTripSegment],
        details: &HashMap<TripId, TripDetails>,
    ) -> Result<usize, AppendError> {
        let rows: Vec<MotisCaptureRow> = segments
            .iter()
            .map(|segment| {
                MotisCaptureRow::of(
                    self.source_id.clone(),
                    segment_row(captured_at, segment, details),
                )
            })
            .collect();
        self.root
            .rows_of::<MotisCaptureRow>()
            .on_date(captured_at.date_naive())
            .expect("a date partition")
            .append_rows(captured_at, &rows)
            .await
    }
}

pub(crate) struct Everywhere(Country);

impl Countries for Everywhere {
    fn containing(&self, _point: Point<f64>) -> Option<Country> {
        Some(self.0)
    }
}

pub(crate) struct Nowhere;

impl Countries for Nowhere {
    fn containing(&self, _point: Point<f64>) -> Option<Country> {
        None
    }
}

pub(crate) fn germany() -> Everywhere {
    Everywhere(Country::Germany)
}

pub(crate) fn source(feed: Feed) -> MotisSource {
    MotisSource {
        base_url: Url::parse("http://127.0.0.1:8080").expect("a URL"),
        motis_version: "v2.11.3".parse().expect("a version"),
        feed,
        area: None,
        label: None,
    }
}

pub(crate) fn trips() -> Vec<TripSegment> {
    serde_json::from_str(TRIPS).expect("parse fixture")
}

pub(crate) fn regional_trips() -> Vec<TripSegment> {
    trips()
        .into_iter()
        .map(|segment| TripSegment {
            mode: Mode::RegionalRail,
            ..segment
        })
        .collect()
}
