use std::time::SystemTime;

use bytes::Bytes;
use ferrostar::{models::{GeographicCoordinate, Route, UserLocation, Waypoint, WaypointKind}, routing_adapters::{osrm::OsrmResponseParser, valhalla::ValhallaHttpRequestGenerator, RouteRequest, RouteRequestGenerator, RouteResponseParser}};
use geo::{coord, Coord, LineString};
use reqwest::header::HeaderMap;
use url::Url;

#[derive(clap:: ValueEnum, Clone, Debug)]
pub enum Server {
    Default,
    EU
}

impl Default for Server {
    fn default() -> Self {
        Server::Default
    }
}

impl ToString for Server {
    fn to_string(&self) -> String {
        match self {
            Server::Default => "default".into(),
            Server::EU => "eu".into(),
        }
    }
}

impl Server {
    fn endpoint_base(&self) -> Url {
        match self {
            Server::Default => Url::parse("https://api.stadiamaps.com").unwrap(),
            Server::EU => Url::parse("https://api-eu.stadiamaps.com").unwrap(),
        }
    }
}

#[derive(clap:: ValueEnum, Clone, Debug)]
pub enum Profile {
    Auto,
    Pedestrian,
}

impl Into<String> for Profile {
    fn into(self) -> String {
        (&self as &Profile).into()
    }
}

impl Into<String> for &Profile {
    fn into(self) -> String {
        match self {
            Profile::Auto => "auto".into(),
            Profile::Pedestrian => "pedestrian".into(),
        }
    }
}

pub struct StandardRouting {
    route_url: Url,
}

impl StandardRouting {
    pub fn new(api_key: &str, server: Server) -> Result<Self, Box<dyn std::error::Error>> {
        let mut route_url = server.endpoint_base().join("/route/v1")?;
        let authenticated_route_url = route_url
            .query_pairs_mut()
            .append_pair("api_key", api_key)
            .finish();
        Ok(StandardRouting {
            route_url: authenticated_route_url.clone(),
        })
    }

    pub async fn find_route(
        &self,
        start: &Coord,
        end: &Coord,
        profile: &Profile,
    ) -> Result<LineString, Box<dyn std::error::Error>> {

        let generator = ValhallaHttpRequestGenerator::new(
            self.route_url.to_string().clone(),
            profile.into(),
            None,
        );

        let user_location = UserLocation {
            coordinates: GeographicCoordinate {
                lat: start.y,
                lng: start.x,
            },
            horizontal_accuracy: 1.0,
            course_over_ground: None,
            timestamp: SystemTime::now(),
            speed: None,
        };

        let waypoints: Vec<Waypoint> = vec![Waypoint {
            coordinate: GeographicCoordinate {
                lat: end.y,
                lng: end.x,
            },
            kind: WaypointKind::Break,
        }];

        let RouteRequest::HttpPost { url, body, headers } =
            generator.generate_request(user_location, waypoints)?;

        let client = reqwest::Client::new();
        let response = client
            .post(url)
            .body(body)
            .headers(HeaderMap::try_from(&headers)?)
            .send()
            .await
            .map_err(|e| format!("Error whilst getting Response: {:?}", e))?;

        let content = response.bytes().await
            .map_err(|e| format!("Error whilst getting bytes: {:?}", e))?;
        let routes = parse_route(content)?;

        let route = routes.first().unwrap();
        let route_line = LineString::new(
            route
                .geometry
                .iter()
                .map(|c| coord!(x: c.lng, y: c.lat))
                .collect(),
        );

        Ok(route_line)
    }
}

fn parse_route(content: Bytes) -> Result<Vec<Route>, Box<dyn std::error::Error>> {
    let polyline_precision = 6;
    let as_vec = content.to_vec();
    match OsrmResponseParser::new(polyline_precision).parse_response(as_vec.clone()) {
        Ok(routes) => Ok(routes),
        Err(e) => Err(format!("Error whilst parsing response: {:?}", e).into()),
    }
}