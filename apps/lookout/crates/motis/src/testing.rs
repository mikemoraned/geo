#![cfg(test)]

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

use chrono::{DateTime, Utc};
use geo_types::{LineString, Point};
use medallion::{Countries, Country, Root};
use medallion_model::{Feed, MotisPollRowV2, MotisSource, MotisSourceId, TripId};
use url::Url;

use crate::api::types::{Mode, TripSegment};
use crate::client::TripDetails;
use crate::landing::poll_row;
use crate::landing_to_bronze_promotion::{PromotionError, write_polls};
use crate::segment::SingleTripSegment;

pub(crate) const TRIPS: &str = include_str!("../tests/fixtures/trips.json");

static NEXT_WRITE: AtomicI64 = AtomicI64::new(0);

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
    ) -> Result<usize, PromotionError> {
        let polls: Vec<MotisPollRowV2> = segments
            .iter()
            .map(|segment| {
                poll_row(self.source_id.clone(), captured_at, segment, details)
                    .expect("a fixture's polyline is valid")
            })
            .collect();
        let written_at =
            DateTime::from_timestamp_millis(NEXT_WRITE.fetch_add(1, Ordering::Relaxed))
                .expect("a small count of milliseconds is an instant");
        write_polls(&self.root, written_at, &polls).await?;
        Ok(polls.len())
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

#[derive(Default)]
pub(crate) struct PretendEastOfGreenwichIsGermany {
    lookups: Cell<usize>,
    coverings: Cell<usize>,
}

impl PretendEastOfGreenwichIsGermany {
    pub(crate) fn lookups(&self) -> usize {
        self.lookups.get()
    }

    pub(crate) fn coverings(&self) -> usize {
        self.coverings.get()
    }
}

impl Countries for PretendEastOfGreenwichIsGermany {
    fn containing(&self, point: Point<f64>) -> Option<Country> {
        self.lookups.set(self.lookups.get() + 1);
        (point.x() > 0.0).then_some(Country::Germany)
    }

    fn covering(&self, line: &LineString<f64>) -> Option<Country> {
        self.coverings.set(self.coverings.get() + 1);
        line.points()
            .all(|point| point.x() > 0.0)
            .then_some(Country::Germany)
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
