use chrono::{DateTime, Utc};
use geo_types::LineString;
use medallion_model::{EmptyTripId, TripId};

use crate::api::types::{TripInfo, TripSegment};

pub const POLYLINE_PRECISION: u32 = 5;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("decoding polyline: {0}")]
pub struct NotAPolyline(String);

pub fn decode_polyline(encoded: &str) -> Result<LineString<f64>, NotAPolyline> {
    polyline::decode_polyline(encoded, POLYLINE_PRECISION).map_err(|e| NotAPolyline(e.to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NoTripOfItsOwn {
    #[error("the segment leaving {from} at {departure} carries {trips} trips, not one")]
    NotOneTrip {
        from: String,
        departure: DateTime<Utc>,
        trips: usize,
    },
    #[error("the segment leaving {from} at {departure} names its trip with an empty id")]
    EmptyId {
        from: String,
        departure: DateTime<Utc>,
    },
}

impl TripSegment {
    pub fn trip(&self) -> Result<&TripInfo, NoTripOfItsOwn> {
        match self.trips.as_slice() {
            [trip] => Ok(trip),
            trips => Err(NoTripOfItsOwn::NotOneTrip {
                from: self.leaving_from(),
                departure: self.departure,
                trips: trips.len(),
            }),
        }
    }

    pub fn trip_id(&self) -> Result<TripId, NoTripOfItsOwn> {
        TripId::new(self.trip()?.trip_id.clone()).map_err(|EmptyTripId| NoTripOfItsOwn::EmptyId {
            from: self.leaving_from(),
            departure: self.departure,
        })
    }

    pub fn line(&self) -> Result<LineString<f64>, NotAPolyline> {
        decode_polyline(&self.polyline)
    }

    fn leaving_from(&self) -> String {
        self.from
            .stop_id
            .clone()
            .unwrap_or_else(|| self.from.name.clone())
    }
}

#[derive(Debug, Clone)]
pub struct SingleTripSegment {
    trip_id: TripId,
    trip: TripInfo,
    segment: TripSegment,
}

impl SingleTripSegment {
    pub fn new(segment: TripSegment) -> Result<Self, NoTripOfItsOwn> {
        let trip = segment.trip()?.clone();
        let trip_id = segment.trip_id()?;
        Ok(Self {
            trip_id,
            trip,
            segment,
        })
    }

    pub fn trip_id(&self) -> &TripId {
        &self.trip_id
    }

    pub fn trip(&self) -> &TripInfo {
        &self.trip
    }

    pub fn segment(&self) -> &TripSegment {
        &self.segment
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments() -> Vec<TripSegment> {
        serde_json::from_str(include_str!("../tests/fixtures/trips.json")).expect("parse fixture")
    }

    #[test]
    fn a_segment_with_one_trip_names_it() {
        let segment = segments().remove(0);

        assert_eq!(
            segment.trip_id().expect("one trip").as_str(),
            segment.trips[0].trip_id
        );
    }

    #[test]
    fn a_segment_with_no_trip_is_refused() {
        let mut segment = segments().remove(0);
        segment.trips.clear();

        assert!(matches!(
            segment.trip(),
            Err(NoTripOfItsOwn::NotOneTrip { trips: 0, .. })
        ));
    }

    #[test]
    fn a_segment_with_two_trips_is_refused() {
        let mut segment = segments().remove(0);
        segment.trips.push(segment.trips[0].clone());

        assert!(matches!(
            segment.trip(),
            Err(NoTripOfItsOwn::NotOneTrip { trips: 2, .. })
        ));
    }

    #[test]
    fn a_segment_whose_trip_has_an_empty_id_is_refused() {
        let mut segment = segments().remove(0);
        segment.trips[0].trip_id.clear();

        assert!(matches!(
            segment.trip_id(),
            Err(NoTripOfItsOwn::EmptyId { .. })
        ));
    }
}
