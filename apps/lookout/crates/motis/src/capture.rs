use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use geo::{BoundingRect, PreparedGeometry, Relate};
use geo_types::{Geometry, GeometryCollection, LineString, Rect};
use medallion_model::TripId;
use tokio::time::Instant;

use crate::api::types::{Mode, TripSegment};
use crate::bronze::{BronzeError, SegmentLog};
use crate::client::{MotisClient, MotisError};
use crate::details::TripDetailsCache;
use crate::span::Span;
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
    shapes: Vec<PreparedGeometry<'static, Geometry<f64>>>,
}

impl Coverage {
    pub fn bbox(bbox: Rect<f64>) -> Self {
        Self {
            bbox,
            shapes: vec![PreparedGeometry::from(Geometry::Rect(bbox))],
        }
    }

    pub fn shapes(shapes: Vec<Geometry<f64>>) -> Option<Self> {
        let bbox = GeometryCollection::new_from(shapes.clone()).bounding_rect()?;
        Some(Self {
            bbox,
            shapes: shapes.into_iter().map(PreparedGeometry::from).collect(),
        })
    }

    fn touches(&self, line: &LineString<f64>) -> bool {
        self.shapes
            .iter()
            .any(|shape| shape.relate(line).is_intersects())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub segments: usize,
    pub outside: usize,
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
    Log(#[from] BronzeError),
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
        let mut unresolved = Vec::new();
        let mut captured: Vec<TripSegment> = Vec::new();
        let mut trip_ids: HashSet<TripId> = HashSet::new();
        let mut outside = 0;
        let mut map_trips = Tally::default();
        let in_bbox = map_trips
            .time(
                self.client
                    .trips_in_bbox(&coverage.bbox, &window, self.zoom),
            )
            .await?;
        let filtering = Instant::now();
        for segment in in_bbox.into_iter().filter(|segment| is_rail(&segment.mode)) {
            match sorted(coverage, &segment) {
                Sorted::Kept(trip_id) => {
                    trip_ids.insert(trip_id);
                    captured.push(segment);
                }
                Sorted::Outside => outside += 1,
                Sorted::Unresolved(reason) => unresolved.push(Unresolved::Segment { reason }),
            }
        }
        let filter_took = filtering.elapsed();

        let resolution = self.details.resolve(&self.client, &trip_ids).await;
        let requests = Requests {
            map_trips,
            trip: resolution.lookups,
        };
        unresolved.extend(resolution.failed.into_iter().map(|(trip_id, err)| {
            Unresolved::Details {
                trip_id,
                reason: err.to_string(),
            }
        }));
        let writing = Instant::now();
        let segments = self.log.append(now, &captured, &resolution.details).await?;
        let log_took = writing.elapsed();

        Ok(Captured {
            segments,
            outside,
            unresolved,
            requests,
            filter_took,
            log_took,
        })
    }
}

enum Sorted {
    Kept(TripId),
    Outside,
    Unresolved(String),
}

fn sorted(coverage: &Coverage, segment: &TripSegment) -> Sorted {
    match segment.line() {
        Err(err) => Sorted::Unresolved(err.to_string()),
        Ok(line) if !coverage.touches(&line) => Sorted::Outside,
        Ok(_) => match segment.trip_id() {
            Ok(trip_id) => Sorted::Kept(trip_id),
            Err(err) => Sorted::Unresolved(err.to_string()),
        },
    }
}

fn is_rail(mode: &Mode) -> bool {
    matches!(
        mode,
        Mode::HighspeedRail
            | Mode::LongDistance
            | Mode::NightRail
            | Mode::RegionalFastRail
            | Mode::RegionalRail
            | Mode::Rail
    )
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

    const TRIPS: &str = include_str!("../tests/fixtures/trips.json");
    const TRIP: &str = include_str!("../tests/fixtures/trip.json");

    async fn motis() -> MockServer {
        let server = MockServer::start().await;
        for (route, body) in [("/api/v6/map/trips", TRIPS), ("/api/v6/trip", TRIP)] {
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
            motis_version: "v2.11.3".parse().expect("a version"),
            feed: Feed::Local,
            area: None,
            label: None,
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

    #[test]
    fn a_coverage_of_shapes_queries_their_bounding_box() {
        let coverage = Coverage::shapes(vec![square(8.0, 50.0, 1.0), square(10.0, 52.0, 1.0)])
            .expect("shapes");

        assert_eq!(
            coverage.bbox,
            Rect::new(Coord { x: 8.0, y: 50.0 }, Coord { x: 11.0, y: 53.0 })
        );
    }

    #[test]
    fn no_shapes_cover_nothing() {
        assert!(Coverage::shapes(Vec::new()).is_none());
    }

    #[tokio::test]
    async fn a_leg_touching_the_shapes_is_captured() {
        let server = motis().await;
        let store = tempfile::tempdir().expect("tempdir");
        let around_frankfurt = Coverage::shapes(vec![square(8.0, 49.5, 2.0)]).expect("shapes");

        let captured = capture(&server, store.path())
            .capture(chrono::Utc::now(), &around_frankfurt)
            .await
            .expect("capture");

        assert_eq!((captured.segments, captured.outside), (1, 0));
    }

    #[tokio::test]
    async fn a_leg_inside_the_bounding_box_but_clear_of_the_shapes_is_outside() {
        let server = motis().await;
        let store = tempfile::tempdir().expect("tempdir");
        let elsewhere = Coverage::shapes(vec![square(0.0, 0.0, 1.0)]).expect("shapes");

        let captured = capture(&server, store.path())
            .capture(chrono::Utc::now(), &elsewhere)
            .await
            .expect("capture");

        assert_eq!((captured.segments, captured.outside), (0, 1));
    }

    #[tokio::test]
    async fn a_leg_touching_the_bounding_box_is_captured() {
        let server = motis().await;
        let store = tempfile::tempdir().expect("tempdir");
        let around_frankfurt = Coverage::bbox(Rect::new(
            Coord { x: 8.0, y: 49.5 },
            Coord { x: 10.0, y: 51.5 },
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
        let around_frankfurt = Coverage::shapes(vec![square(8.0, 49.5, 2.0)]).expect("shapes");
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
        let around_frankfurt = Coverage::shapes(vec![square(8.0, 49.5, 2.0)]).expect("shapes");
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
}
