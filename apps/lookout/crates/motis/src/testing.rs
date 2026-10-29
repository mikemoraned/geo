#![cfg(test)]

use geo_types::Point;
use medallion::{Countries, Country};
use medallion_model::{Feed, MotisSource};
use url::Url;

use crate::api::types::{Mode, TripSegment};

pub(crate) const TRIPS: &str = include_str!("../tests/fixtures/trips.json");

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
