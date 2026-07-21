//! A thin wrapper over the generated `motis-openapi-progenitor` client that queries the
//! Motis `map/trips` endpoint for train trips within a bounding box and time window.

use chrono::{DateTime, Duration, Timelike, Utc};
use geo_types::Rect;
use motis_openapi_progenitor::{
    types::{Itinerary, TripSegment},
    Client,
};

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

/// The operating agency of a trip, as reported by the Motis `trip` endpoint. It is the
/// only discriminator between long-distance and regional rail: the gtfs.de free feed
/// types every rail service as `route_type=2`, so `mode` reports an ICE as
/// `REGIONAL_RAIL` — but its agency is `DB Fernverkehr AG`. Either field may be absent
/// even when a trip resolves.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Agency {
    pub id: Option<String>,
    pub name: Option<String>,
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

    /// The operating [`Agency`] of `trip_id`, from the Motis `trip` endpoint — the only
    /// place agency is exposed (`map/trips` segments don't carry it). Interlined legs are
    /// kept separate (`join_interlined_legs=false`) so a stay-seated trip isn't collapsed
    /// into one leg spanning multiple agencies; the agency is taken from the first leg
    /// that names one. `Ok(None)` when the trip resolves but no leg names an agency.
    pub async fn trip_agency(&self, trip_id: &str) -> Result<Option<Agency>, MotisError> {
        let itinerary = self
            .inner
            .trip()
            .trip_id(trip_id)
            .join_interlined_legs(false)
            .send()
            .await?
            .into_inner();
        Ok(agency_of(itinerary))
    }
}

/// The first agency named across an itinerary's legs, if any.
fn agency_of(itinerary: Itinerary) -> Option<Agency> {
    itinerary
        .legs
        .into_iter()
        .find(|leg| leg.agency_name.is_some() || leg.agency_id.is_some())
        .map(|leg| Agency {
            id: leg.agency_id,
            name: leg.agency_name,
        })
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

    #[test]
    fn agency_taken_from_first_leg_that_names_one() {
        let itinerary: Itinerary =
            serde_json::from_str(include_str!("../tests/fixtures/trip.json"))
                .expect("parse trip fixture");
        let agency = agency_of(itinerary).expect("fixture leg names an agency");
        assert_eq!(agency.name.as_deref(), Some("DB Regio AG Mitte Region Hessen"));
        assert_eq!(agency.id.as_deref(), Some("292"));
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

        // The first segment's trip resolves to a named agency via the `trip` endpoint.
        let trip_id = segments[0].trips[0].trip_id.clone();
        let agency = client
            .trip_agency(&trip_id)
            .await
            .expect("resolve trip agency")
            .expect("trip names an agency");
        assert!(
            agency.name.is_some() || agency.id.is_some(),
            "expected an agency name or id"
        );
    }
}
