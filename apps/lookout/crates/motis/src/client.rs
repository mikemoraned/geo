//! A thin wrapper over the generated `motis-openapi-progenitor` client that queries the
//! Motis `map/trips` endpoint for train trips within a bounding box and time window.

use chrono::{DateTime, Duration, Timelike, Utc};
use geo_types::Rect;
use motis_openapi_progenitor::{types::TripSegment, Client};

/// Where the local Motis server listens unless overridden. Uses `127.0.0.1` rather than
/// `localhost`: Motis binds IPv4 `0.0.0.0`, but `localhost` resolves to IPv6 `::1` first,
/// which the server never accepts.
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8080";

/// A failure querying the Motis server.
#[derive(Debug, thiserror::Error)]
pub enum MotisError {
    #[error("motis trips request failed: {0}")]
    Request(#[from] motis_openapi_progenitor::Error<()>),
}

/// The `[start, end]` time span a `map/trips` query covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl TimeWindow {
    /// A window centred on `now`, reaching `half_width` either side of it.
    pub fn around(now: DateTime<Utc>, half_width: Duration) -> Self {
        Self {
            start: now - half_width,
            end: now + half_width,
        }
    }
}

/// A client for the Motis `map/trips` endpoint.
#[derive(Debug, Clone)]
pub struct MotisClient {
    inner: Client,
}

impl Default for MotisClient {
    fn default() -> Self {
        Self::new(DEFAULT_BASE_URL)
    }
}

impl MotisClient {
    /// A client talking to the Motis server at `base_url`.
    pub fn new(base_url: &str) -> Self {
        Self {
            inner: Client::new(base_url),
        }
    }

    /// The trip segments Motis reports within `bbox` over `window`, at the given `zoom`
    /// (higher zoom widens the modes returned — subway/tram/bus on top of rail).
    pub async fn trips_in_bbox(
        &self,
        bbox: &Rect<f64>,
        window: &TimeWindow,
        zoom: f64,
    ) -> Result<Vec<TripSegment>, MotisError> {
        let (min, max) = bbox_corners(bbox);
        let response = self
            .inner
            .trips()
            .zoom(zoom)
            .min(min)
            .max(max)
            .start_time(whole_second(window.start))
            .end_time(whole_second(window.end))
            .send()
            .await?;
        Ok(response.into_inner())
    }
}

/// Truncate to a whole second. Motis `map/trips` mis-parses fractional-second RFC3339
/// bounds — the response swings between empty and wildly oversized — whereas
/// whole-second bounds return a stable result. Progenitor serialises `DateTime<Utc>`
/// with sub-second precision, so the bounds are truncated here before the query.
fn whole_second(t: DateTime<Utc>) -> DateTime<Utc> {
    t.with_nanosecond(0)
        .expect("zero nanoseconds is always a valid time")
}

/// Map a box to Motis `min`/`max` params: `min` is the SW corner (`min_lat,min_lon`),
/// `max` the NE corner (`max_lat,max_lon`). The [`Rect`] is `(lon, lat)` in `(x, y)`.
fn bbox_corners(bbox: &Rect<f64>) -> (String, String) {
    (
        format!("{},{}", bbox.min().y, bbox.min().x),
        format!("{},{}", bbox.max().y, bbox.max().x),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo_types::Coord;

    #[test]
    fn bbox_maps_to_sw_and_ne_corner_strings() {
        let bbox = Rect::new(
            Coord { x: 8.4, y: 49.5 },
            Coord { x: 9.75, y: 50.25 },
        );
        let (min, max) = bbox_corners(&bbox);
        assert_eq!(min, "49.5,8.4");
        assert_eq!(max, "50.25,9.75");
    }

    #[test]
    fn whole_second_drops_sub_second_precision() {
        let t = DateTime::from_timestamp(1_700_000_000, 738_002_000).unwrap();
        assert_eq!(whole_second(t), DateTime::from_timestamp(1_700_000_000, 0).unwrap());
    }

    #[test]
    fn window_around_now_is_symmetric() {
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let window = TimeWindow::around(now, Duration::minutes(5));
        assert_eq!(window.start, now - Duration::minutes(5));
        assert_eq!(window.end, now + Duration::minutes(5));
        assert_eq!(window.end - window.start, Duration::minutes(10));
    }

    /// Hits the Motis server on `localhost:8080`; run with the server up via
    /// `just end_to_end_test`.
    #[tokio::test]
    async fn trips_in_bbox_hits_local_server_end_to_end() {
        let client = MotisClient::default();
        // A box over the Frankfurt area, a short window around now.
        let bbox = Rect::new(Coord { x: 8.4, y: 49.9 }, Coord { x: 9.0, y: 50.3 });
        let window = TimeWindow::around(Utc::now(), Duration::minutes(5));
        let segments = client
            .trips_in_bbox(&bbox, &window, 8.0)
            .await
            .expect("query the local Motis server");
        assert!(
            !segments.is_empty(),
            "expected some trip segments in the Frankfurt box"
        );
    }
}
