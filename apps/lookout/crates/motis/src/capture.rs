use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use domain::Span;
use geo::{BoundingRect, PreparedGeometry, Relate, unary_union};
use geo_types::{Geometry, GeometryCollection, LineString, MultiPolygon, Rect};
use medallion_model::{RailMode, TripId};
use tokio::time::Instant;

use crate::api::types::{Mode, TripSegment};
use crate::client::{MotisClient, MotisError};
use crate::details::TripDetailsCache;
use crate::landing::{SegmentLog, SegmentLogError};
use crate::segment::SingleTripSegment;
use crate::tally::Tally;
use crate::zoom::Zoom;

#[derive(Debug)]
pub struct Capture {
    client: MotisClient,
    details: TripDetailsCache,
    log: SegmentLog,
    zoom: Zoom,
    poll_interval: Span,
}

#[derive(Debug, Clone)]
pub struct Coverage {
    bbox: Rect<f64>,
    area: PreparedGeometry<'static, Geometry<f64>>,
    country: Option<PreparedGeometry<'static, Geometry<f64>>>,
}

impl Coverage {
    pub fn bbox(bbox: Rect<f64>) -> Self {
        Self {
            bbox,
            area: PreparedGeometry::from(Geometry::Rect(bbox)),
            country: None,
        }
    }

    pub fn area(area: Vec<Geometry<f64>>, country: Vec<Geometry<f64>>) -> Option<Self> {
        let bbox = GeometryCollection::new_from(area.clone()).bounding_rect()?;
        Some(Self {
            bbox,
            area: union(area),
            country: Some(union(country)),
        })
    }

    fn keeps(&self, line: &LineString<f64>) -> bool {
        self.area.relate(line).is_intersects()
            && self
                .country
                .as_ref()
                .is_none_or(|country| country.relate(line).is_covers())
    }
}

fn union(shapes: Vec<Geometry<f64>>) -> PreparedGeometry<'static, Geometry<f64>> {
    let polygons: Vec<MultiPolygon<f64>> = shapes.into_iter().filter_map(polygonal).collect();
    PreparedGeometry::from(Geometry::MultiPolygon(unary_union(&polygons)))
}

fn polygonal(shape: Geometry<f64>) -> Option<MultiPolygon<f64>> {
    match shape {
        Geometry::Polygon(polygon) => Some(MultiPolygon::new(vec![polygon])),
        Geometry::MultiPolygon(polygons) => Some(polygons),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Captured {
    pub segments: usize,
    pub outside: usize,
    pub no_polyline: usize,
    pub invalid_polylines: Vec<String>,
    pub unresolved: Vec<Unresolved>,
    pub requests: Requests,
    pub filter_took: Duration,
    pub log_took: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Requests {
    pub map_trips: Tally,
    pub trip: Tally,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unresolved {
    Segment { reason: String },
    Details { trip_id: TripId, reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("querying motis: {0}")]
    Motis(#[from] MotisError),
    #[error("writing capture log: {0}")]
    Log(#[from] SegmentLogError),
}

impl Capture {
    pub fn new(client: MotisClient, log: SegmentLog, zoom: Zoom, poll_interval: Span) -> Self {
        Self {
            client,
            details: TripDetailsCache::default(),
            log,
            zoom,
            poll_interval,
        }
    }

    pub async fn capture(
        &mut self,
        now: DateTime<Utc>,
        coverage: &Coverage,
    ) -> Result<Captured, CaptureError> {
        let half = self.poll_interval.time_delta() / 2;
        let window = now - half..now + half;
        let mut captured = Captured::default();
        let mut kept_segments: Vec<SingleTripSegment> = Vec::new();
        let mut trip_ids: HashSet<TripId> = HashSet::new();
        let in_bbox = captured
            .requests
            .map_trips
            .time(
                self.client
                    .trips_in_bbox(&coverage.bbox, &window, self.zoom),
            )
            .await?;
        let filtering = Instant::now();
        for segment in in_bbox.into_iter().filter(|segment| is_rail(&segment.mode)) {
            match categorise(coverage, segment) {
                Category::Kept(kept) => {
                    trip_ids.insert(kept.trip_id().clone());
                    kept_segments.push(*kept);
                }
                Category::Outside => captured.outside += 1,
                Category::NoPolyline => captured.no_polyline += 1,
                Category::InvalidPolyline(reason) => captured.invalid_polylines.push(reason),
                Category::Unresolved(reason) => {
                    captured.unresolved.push(Unresolved::Segment { reason });
                }
            }
        }
        captured.filter_took = filtering.elapsed();

        let resolution = self.details.resolve(&self.client, &trip_ids).await;
        captured.requests.trip = resolution.lookups;
        captured
            .unresolved
            .extend(
                resolution
                    .failed
                    .into_iter()
                    .map(|(trip_id, err)| Unresolved::Details {
                        trip_id,
                        reason: err.to_string(),
                    }),
            );
        let writing = Instant::now();
        captured.segments = self
            .log
            .append(now, &kept_segments, &resolution.details)
            .await?;
        captured.log_took = writing.elapsed();

        Ok(captured)
    }
}

enum Category {
    Kept(Box<SingleTripSegment>),
    Outside,
    NoPolyline,
    InvalidPolyline(String),
    Unresolved(String),
}

fn categorise(coverage: &Coverage, segment: TripSegment) -> Category {
    if segment.polyline.is_empty() {
        return Category::NoPolyline;
    }
    match segment.line() {
        Err(err) => Category::InvalidPolyline(err.to_string()),
        Ok(line) if !coverage.keeps(&line) => Category::Outside,
        Ok(_) => match SingleTripSegment::new(segment) {
            Ok(kept) => Category::Kept(Box::new(kept)),
            Err(err) => Category::Unresolved(err.to_string()),
        },
    }
}

fn is_rail(mode: &Mode) -> bool {
    mode.to_string().parse::<RailMode>().is_ok()
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use geo_types::{Coord, polygon};
    use medallion::Root;
    use medallion_model::{Feed, MotisSource};
    use url::Url;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::testing::{self, TRIPS};

    const TRIP: &str = include_str!("../tests/fixtures/trip.json");

    async fn motis() -> MockServer {
        motis_serving(TRIPS).await
    }

    async fn motis_serving(trips: &str) -> MockServer {
        let server = MockServer::start().await;
        for (route, body) in [("/api/v6/map/trips", trips), ("/api/v6/trip", TRIP)] {
            Mock::given(method("GET"))
                .and(path(route))
                .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
                .mount(&server)
                .await;
        }
        server
    }

    fn capture(server: &MockServer, store: &std::path::Path) -> Capture {
        let source = MotisSource {
            base_url: Url::parse(&server.uri()).expect("a URL"),
            ..testing::source(Feed::Local)
        };
        Capture::new(
            MotisClient::new(&source.base_url),
            SegmentLog::new(Root::new(store), source.id()),
            Zoom::default(),
            "10m".parse().expect("a span"),
        )
    }

    fn square(min_lon: f64, min_lat: f64, size: f64) -> Geometry<f64> {
        Geometry::Polygon(polygon![
            (x: min_lon, y: min_lat),
            (x: min_lon + size, y: min_lat),
            (x: min_lon + size, y: min_lat + size),
            (x: min_lon, y: min_lat + size),
        ])
    }

    fn whole_country(shapes: Vec<Geometry<f64>>) -> Coverage {
        Coverage::area(shapes.clone(), shapes).expect("shapes")
    }

    async fn categorised_by(coverage: &Coverage) -> (usize, usize) {
        let server = motis().await;
        let store = tempfile::tempdir().expect("tempdir");
        let captured = capture(&server, store.path())
            .capture(chrono::Utc::now(), coverage)
            .await
            .expect("capture");
        (captured.segments, captured.outside)
    }

    #[test]
    fn a_coverage_of_an_area_queries_its_bounding_box() {
        let coverage = Coverage::area(
            vec![square(8.0, 50.0, 1.0), square(10.0, 52.0, 1.0)],
            vec![square(0.0, 40.0, 20.0)],
        )
        .expect("shapes");

        assert_eq!(
            coverage.bbox,
            Rect::new(Coord { x: 8.0, y: 50.0 }, Coord { x: 11.0, y: 53.0 })
        );
    }

    #[test]
    fn no_area_covers_nothing() {
        assert!(Coverage::area(Vec::new(), vec![square(0.0, 40.0, 20.0)]).is_none());
    }

    #[tokio::test]
    async fn a_leg_within_the_country_is_captured() {
        assert_eq!(
            categorised_by(&whole_country(vec![square(8.0, 49.5, 2.0)])).await,
            (1, 0)
        );
    }

    fn rail_leg() -> serde_json::Value {
        serde_json::from_str::<Vec<serde_json::Value>>(TRIPS)
            .expect("parse fixture")
            .into_iter()
            .find(|segment| segment["mode"] == "REGIONAL_RAIL")
            .expect("a rail segment")
    }

    async fn captured_with_one_leg_holding(polyline: &str) -> Captured {
        let rail = rail_leg();
        let mut other = rail.clone();
        other["polyline"] = serde_json::json!(polyline);
        let server = motis_serving(&serde_json::json!([other, rail]).to_string()).await;
        let store = tempfile::tempdir().expect("tempdir");

        capture(&server, store.path())
            .capture(
                chrono::Utc::now(),
                &whole_country(vec![square(8.0, 49.5, 2.0)]),
            )
            .await
            .expect("capture")
    }

    #[tokio::test]
    async fn a_leg_with_no_polyline_is_counted_as_that_and_the_other_rows_land() {
        let captured = captured_with_one_leg_holding("").await;

        assert_eq!(captured.segments, 1);
        assert_eq!(captured.no_polyline, 1);
        assert_eq!(captured.outside, 0);
        assert!(captured.invalid_polylines.is_empty());
    }

    #[tokio::test]
    async fn a_leg_whose_polyline_the_decoder_refuses_is_ignored_and_the_other_rows_land() {
        let captured = captured_with_one_leg_holding("!!!").await;

        assert_eq!(captured.segments, 1);
        assert_eq!(captured.invalid_polylines.len(), 1);
        assert_eq!(captured.no_polyline, 0);
        assert!(captured.unresolved.is_empty());
    }

    #[tokio::test]
    async fn a_leg_whose_polyline_holds_a_character_above_126_is_ignored_and_the_other_rows_land() {
        let rail = rail_leg();
        let polyline = rail["polyline"].as_str().expect("a polyline");

        let captured = captured_with_one_leg_holding(&format!("{polyline}\u{7f}??")).await;

        assert_eq!(captured.segments, 1);
        assert_eq!(captured.invalid_polylines.len(), 1);
    }

    #[tokio::test]
    async fn a_segment_without_a_trip_of_its_own_is_reported_and_the_rest_of_the_poll_kept() {
        let rail = rail_leg();
        let mut tripless = rail.clone();
        tripless["trips"] = serde_json::json!([]);
        let server = motis_serving(&serde_json::json!([tripless, rail]).to_string()).await;
        let store = tempfile::tempdir().expect("tempdir");

        let captured = capture(&server, store.path())
            .capture(
                chrono::Utc::now(),
                &whole_country(vec![square(8.0, 49.5, 2.0)]),
            )
            .await
            .expect("capture");

        assert_eq!(captured.segments, 1);
        assert!(
            matches!(captured.unresolved.as_slice(), [Unresolved::Segment { .. }]),
            "{:?}",
            captured.unresolved
        );
    }

    #[tokio::test]
    async fn a_leg_leaving_the_country_is_outside() {
        assert_eq!(
            categorised_by(&whole_country(vec![square(8.0, 49.5, 1.0)])).await,
            (0, 1)
        );
    }

    #[tokio::test]
    async fn a_leg_leaving_a_region_but_within_its_country_is_captured() {
        let frankfurt_in_germany =
            Coverage::area(vec![square(8.0, 49.5, 1.0)], vec![square(5.0, 47.0, 10.0)])
                .expect("shapes");

        assert_eq!(categorised_by(&frankfurt_in_germany).await, (1, 0));
    }

    #[tokio::test]
    async fn a_leg_across_two_adjoining_shapes_of_the_country_is_captured() {
        let two_halves = whole_country(vec![square(8.0, 49.5, 1.0), square(8.0, 50.5, 2.0)]);

        assert_eq!(categorised_by(&two_halves).await, (1, 0));
    }

    #[tokio::test]
    async fn a_leg_within_the_country_but_clear_of_the_area_is_outside() {
        let elsewhere_in_the_country = Coverage::area(
            vec![square(0.0, 0.0, 1.0)],
            vec![square(-10.0, -10.0, 80.0)],
        )
        .expect("shapes");

        assert_eq!(categorised_by(&elsewhere_in_the_country).await, (0, 1));
    }

    #[tokio::test]
    async fn a_leg_touching_the_bounding_box_is_captured() {
        let server = motis().await;
        let store = tempfile::tempdir().expect("tempdir");
        let around_frankfurt = Coverage::bbox(Rect::new(
            Coord { x: 8.0, y: 49.5 },
            Coord { x: 9.0, y: 50.5 },
        ));

        let captured = capture(&server, store.path())
            .capture(chrono::Utc::now(), &around_frankfurt)
            .await
            .expect("capture");

        assert_eq!((captured.segments, captured.outside), (1, 0));
    }

    #[tokio::test]
    async fn a_poll_counts_its_requests_of_each_kind() {
        let server = motis().await;
        let store = tempfile::tempdir().expect("tempdir");
        let around_frankfurt = whole_country(vec![square(8.0, 49.5, 2.0)]);
        let mut capture = capture(&server, store.path());

        let first = capture
            .capture(chrono::Utc::now(), &around_frankfurt)
            .await
            .expect("capture");
        let second = capture
            .capture(chrono::Utc::now(), &around_frankfurt)
            .await
            .expect("capture");

        let counts = |captured: &Captured| {
            (
                captured.requests.map_trips.requests,
                captured.requests.trip.requests,
            )
        };
        assert_eq!(counts(&first), (1, 1));
        assert_eq!(counts(&second), (1, 0), "the trip's details are cached");
    }

    #[tokio::test]
    async fn a_poll_asks_for_the_trips_within_its_interval_centred_on_now() {
        let server = motis().await;
        let store = tempfile::tempdir().expect("tempdir");
        let around_frankfurt = whole_country(vec![square(8.0, 49.5, 2.0)]);
        let now: DateTime<Utc> = "2026-10-17T12:00:00Z".parse().expect("an instant");

        capture(&server, store.path())
            .capture(now, &around_frankfurt)
            .await
            .expect("capture");

        let requests = server.received_requests().await.expect("recorded requests");
        let map_trips = requests
            .iter()
            .find(|request| request.url.path() == "/api/v6/map/trips")
            .expect("a map/trips request");
        let instant = |name: &str| -> DateTime<Utc> {
            let (_, value) = map_trips
                .url
                .query_pairs()
                .find(|(key, _)| key == name)
                .expect(name);
            DateTime::parse_from_rfc3339(&value)
                .expect("an RFC 3339 instant")
                .to_utc()
        };
        assert_eq!(
            (instant("startTime"), instant("endTime")),
            (now - TimeDelta::minutes(5), now + TimeDelta::minutes(5))
        );
    }

    #[tokio::test]
    async fn a_leg_clear_of_the_bounding_box_is_outside() {
        let server = motis().await;
        let store = tempfile::tempdir().expect("tempdir");
        let elsewhere = Coverage::bbox(Rect::new(
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
        ));

        let captured = capture(&server, store.path())
            .capture(chrono::Utc::now(), &elsewhere)
            .await
            .expect("capture");

        assert_eq!((captured.segments, captured.outside), (0, 1));
    }

    #[test]
    fn the_capture_keeps_rail_and_drops_every_other_mode() {
        let rail = [
            Mode::HighspeedRail,
            Mode::LongDistance,
            Mode::NightRail,
            Mode::RegionalFastRail,
            Mode::RegionalRail,
            Mode::Rail,
        ];
        for mode in rail {
            assert!(is_rail(&mode), "{mode}");
        }
        for mode in [
            Mode::Suburban,
            Mode::Ferry,
            Mode::Bus,
            Mode::Tram,
            Mode::Walk,
        ] {
            assert!(!is_rail(&mode), "{mode}");
        }
    }
}
