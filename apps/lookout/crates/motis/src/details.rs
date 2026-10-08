use std::collections::{HashMap, HashSet};

use medallion_model::TripId;

use crate::client::{MotisClient, MotisError, TripDetails};

#[derive(Debug, Clone, Default)]
pub struct TripDetailsCache {
    known: HashMap<TripId, TripDetails>,
}

#[derive(Debug)]
pub struct Resolution {
    pub details: HashMap<TripId, TripDetails>,
    pub failed: Vec<(TripId, MotisError)>,
}

impl TripDetailsCache {
    pub async fn resolve(
        &mut self,
        client: &MotisClient,
        trip_ids: &HashSet<TripId>,
    ) -> Resolution {
        let mut failed = Vec::new();
        for trip_id in trip_ids {
            if !self.known.contains_key(trip_id) {
                match client.trip_details(trip_id).await {
                    Ok(details) => {
                        self.known.insert(trip_id.clone(), details);
                    }
                    Err(err) => failed.push((trip_id.clone(), err)),
                }
            }
        }
        let details = trip_ids
            .iter()
            .filter_map(|trip_id| {
                self.known
                    .get(trip_id)
                    .map(|details| (trip_id.clone(), details.clone()))
            })
            .collect();
        Resolution { details, failed }
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::api::types::TripSegment;

    use super::*;

    const TRIP: &str = include_str!("../tests/fixtures/trip.json");

    fn trip_ids() -> HashSet<TripId> {
        serde_json::from_str::<Vec<TripSegment>>(include_str!("../tests/fixtures/trips.json"))
            .expect("parse fixture")
            .iter()
            .map(|segment| segment.trip_id().expect("a segment with one trip"))
            .collect()
    }

    async fn trip_server(status: u16, expected: usize) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v6/trip"))
            .respond_with(ResponseTemplate::new(status).set_body_raw(TRIP, "application/json"))
            .expect(expected as u64)
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn a_trip_is_asked_about_once_however_many_polls_see_it() {
        let server = trip_server(200, trip_ids().len()).await;
        let client = MotisClient::new(&server.uri());
        let mut cache = TripDetailsCache::default();

        let first = cache.resolve(&client, &trip_ids()).await;
        let second = cache.resolve(&client, &trip_ids()).await;

        assert_eq!(first.details.len(), trip_ids().len());
        assert_eq!(second.details, first.details);
        assert!(first.failed.is_empty() && second.failed.is_empty());
    }

    #[tokio::test]
    async fn a_failed_lookup_is_reported_and_asked_again_next_time() {
        let server = trip_server(500, trip_ids().len() * 2).await;
        let client = MotisClient::new(&server.uri());
        let mut cache = TripDetailsCache::default();

        let first = cache.resolve(&client, &trip_ids()).await;
        let second = cache.resolve(&client, &trip_ids()).await;

        for resolution in [first, second] {
            assert!(resolution.details.is_empty());
            let failed: HashSet<TripId> = resolution
                .failed
                .into_iter()
                .map(|(trip_id, _)| trip_id)
                .collect();
            assert_eq!(failed, trip_ids());
        }
    }

    #[tokio::test]
    async fn a_poll_gets_details_for_its_own_trips_alone() {
        let server = trip_server(200, trip_ids().len()).await;
        let client = MotisClient::new(&server.uri());
        let mut cache = TripDetailsCache::default();
        cache.resolve(&client, &trip_ids()).await;

        let one: HashSet<TripId> = trip_ids().into_iter().take(1).collect();
        let resolution = cache.resolve(&client, &one).await;

        assert_eq!(resolution.details.len(), 1);
    }
}
