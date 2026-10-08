use crate::api::{
    Client,
    types::{Itinerary, TripSegment},
};
use std::ops::Range;

use chrono::{DateTime, Timelike, Utc};
use domain::TrainNumber;
use geo_types::Rect;
use medallion_model::{MotisVersion, NotAMotisVersion, TripId};

pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8080";

pub const USER_AGENT: &str = concat!(
    "lookout/",
    env!("CARGO_PKG_VERSION"),
    "+",
    env!("BUILD_GIT_HASH"),
    " (+https://github.com/mikemoraned/geo)"
);

const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
pub enum MotisError {
    #[error("motis request failed: {0}")]
    Request(#[from] crate::api::Error<crate::api::types::Error>),
    #[error("motis reports a version this store cannot record: {0}")]
    Version(#[from] NotAMotisVersion),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agency {
    pub id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TripDetails {
    pub agency: Agency,
    pub train_number: Option<TrainNumber>,
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
        let http = reqwest::ClientBuilder::new()
            .user_agent(USER_AGENT)
            .connect_timeout(TIMEOUT)
            .timeout(TIMEOUT)
            .build()
            .expect("a client with only a user agent and timeouts set builds");
        Self {
            inner: Client::new_with_client(base_url, http),
        }
    }

    pub async fn motis_version(&self) -> Result<MotisVersion, MotisError> {
        let initial = self.inner.initial().send().await?.into_inner();
        Ok(initial.server_config.motis_version.parse()?)
    }

    pub async fn trips_in_bbox(
        &self,
        bbox: &Rect<f64>,
        window: &Range<DateTime<Utc>>,
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

    pub async fn trip_details(&self, trip_id: &TripId) -> Result<TripDetails, MotisError> {
        let itinerary = self
            .inner
            .trip()
            .trip_id(trip_id.as_str())
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
        .unwrap_or(Agency {
            id: None,
            name: None,
        });
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
        let now = Utc::now();
        let window = now - chrono::Duration::minutes(5)..now + chrono::Duration::minutes(5);
        let segments = client
            .trips_in_bbox(&over_frankfurt, &window, 8.0)
            .await
            .expect("query the local Motis server");
        assert!(
            !segments.is_empty(),
            "expected some trip segments in the Frankfurt box"
        );

        let trip_id = segments[0].trip_id().expect("a segment with one trip");
        let details = client
            .trip_details(&trip_id)
            .await
            .expect("resolve trip details");
        assert!(
            details.agency.name.is_some() || details.agency.id.is_some(),
            "expected an agency name or id"
        );
    }

    #[tokio::test]
    async fn every_request_names_lookout_in_its_user_agent() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/api/v1/map/initial"))
            .and(wiremock::matchers::header("user-agent", USER_AGENT))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "lat": 50.7, "lon": 9.9, "zoom": 7.0,
                    "serverConfig": {
                        "motisVersion": "v2.11.3", "hasElevation": false,
                        "hasRoutedTransfers": false, "hasStreetRouting": true,
                        "maxOneToManySize": 128.0, "maxOneToAllTravelTimeLimit": 90.0,
                        "maxPrePostTransitTimeLimit": 3600.0, "maxDirectTimeLimit": 21600.0,
                        "shapesDebugEnabled": false
                    }
                })),
            )
            .mount(&server)
            .await;

        let version = MotisClient::new(&server.uri())
            .motis_version()
            .await
            .expect("the mock answers a request carrying the user agent");

        assert_eq!(version.to_string(), "v2.11.3");
    }

    #[test]
    fn the_user_agent_names_the_project_its_versions_and_where_to_reach_it() {
        assert!(USER_AGENT.starts_with("lookout/"), "{USER_AGENT}");
        assert!(
            USER_AGENT.contains(env!("CARGO_PKG_VERSION")),
            "{USER_AGENT}"
        );
        assert!(USER_AGENT.contains(env!("BUILD_GIT_HASH")), "{USER_AGENT}");
        assert!(
            USER_AGENT.contains("https://github.com/mikemoraned/geo"),
            "{USER_AGENT}"
        );
    }

    #[tokio::test]
    async fn the_local_server_reports_its_version_end_to_end() {
        let version = MotisClient::default()
            .motis_version()
            .await
            .expect("ask the local Motis server for its version");

        assert!(version.to_string().starts_with('v'), "{version}");
    }
}
