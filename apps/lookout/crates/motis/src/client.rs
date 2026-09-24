use chrono::{DateTime, Duration, Timelike, Utc};
use domain::TrainNumber;
use geo_types::Rect;
use motis_openapi_progenitor::{
    Client,
    types::{Itinerary, TripSegment},
};

pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8080";

#[derive(Debug, thiserror::Error)]
pub enum MotisError {
    #[error("motis trips request failed: {0}")]
    Request(#[from] motis_openapi_progenitor::Error<()>),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Agency {
    pub id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TripDetails {
    pub agency: Agency,
    pub train_number: Option<TrainNumber>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl TimeWindow {
    pub fn around(now: DateTime<Utc>, half_width: Duration) -> Self {
        Self {
            start: now - half_width,
            end: now + half_width,
        }
    }
}

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
    pub fn new(base_url: &str) -> Self {
        Self {
            inner: Client::new(base_url),
        }
    }

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

    pub async fn trip_details(&self, trip_id: &str) -> Result<TripDetails, MotisError> {
        let itinerary = self
            .inner
            .trip()
            .trip_id(trip_id)
            .join_interlined_legs(false)
            .send()
            .await?
            .into_inner();
        Ok(details_of(itinerary))
    }
}

fn details_of(itinerary: Itinerary) -> TripDetails {
    let agency = itinerary
        .legs
        .iter()
        .find(|leg| leg.agency_name.is_some() || leg.agency_id.is_some())
        .map(|leg| Agency {
            id: leg.agency_id.clone(),
            name: leg.agency_name.clone(),
        })
        .unwrap_or_default();
    let train_number = itinerary.legs.iter().find_map(|leg| {
        leg.trip_short_name
            .as_deref()
            .and_then(TrainNumber::from_gtfs)
    });
    TripDetails {
        agency,
        train_number,
    }
}

fn whole_second(t: DateTime<Utc>) -> DateTime<Utc> {
    t.with_nanosecond(0)
        .expect("zero nanoseconds is always a valid time")
}

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
        let bbox = Rect::new(Coord { x: 8.4, y: 49.5 }, Coord { x: 9.75, y: 50.25 });
        let (min, max) = bbox_corners(&bbox);
        assert_eq!(min, "49.5,8.4");
        assert_eq!(max, "50.25,9.75");
    }

    #[test]
    fn whole_second_drops_sub_second_precision() {
        let t = DateTime::from_timestamp(1_700_000_000, 738_002_000).unwrap();
        assert_eq!(
            whole_second(t),
            DateTime::from_timestamp(1_700_000_000, 0).unwrap()
        );
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
    fn details_taken_from_legs() {
        let itinerary: Itinerary =
            serde_json::from_str(include_str!("../tests/fixtures/trip.json"))
                .expect("parse trip fixture");
        let details = details_of(itinerary);
        assert_eq!(details.agency.name.as_deref(), Some("DB Fernverkehr AG"));
        assert_eq!(details.agency.id.as_deref(), Some("12681"));
        assert_eq!(
            details.train_number.map(TrainNumber::get),
            Some(2569),
            "the fixture leg carries trip_short_name 002569"
        );
    }

    #[test]
    fn train_number_parses_int_and_rejects_zero_and_non_numeric() {
        assert_eq!(
            TrainNumber::from_gtfs("002569").map(TrainNumber::get),
            Some(2569)
        );
        assert_eq!(
            TrainNumber::from_gtfs("2569").map(TrainNumber::get),
            Some(2569)
        );
        assert_eq!(TrainNumber::from_gtfs("000000"), None);
        assert_eq!(TrainNumber::from_gtfs("0"), None);
        assert_eq!(TrainNumber::from_gtfs(""), None);
        assert_eq!(TrainNumber::from_gtfs("ICE"), None);
    }

    #[tokio::test]
    async fn trips_in_bbox_hits_local_server_end_to_end() {
        let client = MotisClient::default();
        let over_frankfurt = Rect::new(Coord { x: 8.4, y: 49.9 }, Coord { x: 9.0, y: 50.3 });
        let window = TimeWindow::around(Utc::now(), Duration::minutes(5));
        let segments = client
            .trips_in_bbox(&over_frankfurt, &window, 8.0)
            .await
            .expect("query the local Motis server");
        assert!(
            !segments.is_empty(),
            "expected some trip segments in the Frankfurt box"
        );

        let trip_id = segments[0].trips[0].trip_id.clone();
        let details = client
            .trip_details(&trip_id)
            .await
            .expect("resolve trip details");
        assert!(
            details.agency.name.is_some() || details.agency.id.is_some(),
            "expected an agency name or id"
        );
    }
}
