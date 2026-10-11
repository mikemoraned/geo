use crate::api::{
    Client,
    types::{Itinerary, TripSegment},
};
use std::ops::Range;

use chrono::{DateTime, SubsecRound, Utc};
use domain::TrainNumber;
use geo_types::{Coord, Rect};
use medallion_model::{MotisVersion, NotAMotisVersion, TripId};
use url::Url;

use crate::zoom::Zoom;

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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
        Self::new(&Url::parse(DEFAULT_BASE_URL).expect("the default base URL is a URL"))
    }
}

impl MotisClient {
    pub fn new(base_url: &Url) -> Self {
        let http = reqwest::ClientBuilder::new()
            .user_agent(USER_AGENT)
            .connect_timeout(TIMEOUT)
            .timeout(TIMEOUT)
            .build()
            .expect("a client with only a user agent and timeouts set builds");
        Self {
            inner: Client::new_with_client(base_url.as_str().trim_end_matches('/'), http),
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
        zoom: Zoom,
    ) -> Result<Vec<TripSegment>, MotisError> {
        let response = self
            .inner
            .trips()
            .zoom(zoom.level())
            .min(lat_lon(bbox.min()))
            .max(lat_lon(bbox.max()))
            .start_time(window.start.trunc_subsecs(0))
            .end_time(window.end.trunc_subsecs(0))
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

fn lat_lon(corner: Coord<f64>) -> String {
    format!("{},{}", corner.y, corner.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_corner_is_written_latitude_first() {
        assert_eq!(lat_lon(Coord { x: 8.4, y: 49.5 }), "49.5,8.4");
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
            .trips_in_bbox(&over_frankfurt, &window, Zoom::default())
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

        let version = MotisClient::new(&Url::parse(&server.uri()).expect("a URL"))
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
