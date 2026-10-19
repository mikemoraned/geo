use std::collections::BTreeMap;
use std::iter::successors;
use std::time::Duration;

use chrono::{DateTime, Utc};
use geo::{Haversine, InterpolatableLine};
use geo_types::{LineString, Point};
use medallion_model::GeometrySource;

use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Leg {
    departure: DateTime<Utc>,
    arrival: DateTime<Utc>,
    duration: Duration,
    shape: LineString<f64>,
    origin: Point<f64>,
    destination: Point<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidLeg {
    #[error("the leg departing at {departure} arrives earlier, at {arrival}")]
    ArrivesBeforeDeparting {
        departure: DateTime<Utc>,
        arrival: DateTime<Utc>,
    },
    #[error("the leg departing at {departure} has a shape of {points} points, not two or more")]
    TooFewPoints {
        departure: DateTime<Utc>,
        points: usize,
    },
}

impl Leg {
    pub fn new(
        departure: DateTime<Utc>,
        arrival: DateTime<Utc>,
        shape: LineString<f64>,
    ) -> Result<Self, InvalidLeg> {
        let duration = (arrival - departure)
            .to_std()
            .map_err(|_| InvalidLeg::ArrivesBeforeDeparting { departure, arrival })?;
        let (origin, destination) = match shape.0.as_slice() {
            [origin, .., destination] => (Point::from(*origin), Point::from(*destination)),
            points => {
                return Err(InvalidLeg::TooFewPoints {
                    departure,
                    points: points.len(),
                });
            }
        };
        Ok(Self {
            departure,
            arrival,
            duration,
            shape,
            origin,
            destination,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalisedLegs {
    legs: Vec<Leg>,
    departure: DateTime<Utc>,
    arrival: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidLegs {
    #[error("there are no legs to normalise")]
    NoLegs,
    #[error("a leg departs at {next_departure}, before the leg before it arrives at {arrival}")]
    Overlapping {
        arrival: DateTime<Utc>,
        next_departure: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Whereabouts {
    Travelling { position: Point<f64> },
    WaitingAtStation { position: Point<f64> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NoWhereabouts {
    #[error("{t} is before the first leg departs, at {departure}")]
    BeforeAllLegs {
        t: DateTime<Utc>,
        departure: DateTime<Utc>,
    },
    #[error("{t} is after the last leg arrives, at {arrival}")]
    AfterAllLegs {
        t: DateTime<Utc>,
        arrival: DateTime<Utc>,
    },
    #[error("the leg travelled at {t} has no point along its shape")]
    NoPointAlongShape { t: DateTime<Utc> },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub t: DateTime<Utc>,
    pub position: Point<f64>,
    pub geometry_source: GeometrySource,
}

impl NormalisedLegs {
    pub fn new(legs: &[Leg]) -> Result<Self, InvalidLegs> {
        let mut legs = legs.to_vec();
        legs.sort_by_key(|leg| (leg.departure, leg.arrival));
        let (Some(first), Some(last)) = (legs.first(), legs.last()) else {
            return Err(InvalidLegs::NoLegs);
        };
        let (departure, arrival) = (first.departure, last.arrival);
        if let Some([leg, next]) = legs
            .windows(2)
            .find(|pair| pair[1].departure < pair[0].arrival)
        {
            return Err(InvalidLegs::Overlapping {
                arrival: leg.arrival,
                next_departure: next.departure,
            });
        }
        Ok(Self {
            legs,
            departure,
            arrival,
        })
    }

    pub fn position_at(&self, t: DateTime<Utc>) -> Result<Whereabouts, NoWhereabouts> {
        if self.arrival < t {
            return Err(NoWhereabouts::AfterAllLegs {
                t,
                arrival: self.arrival,
            });
        }
        let departed = self.legs.partition_point(|leg| leg.departure <= t);
        let leg = self.legs[..departed]
            .last()
            .ok_or(NoWhereabouts::BeforeAllLegs {
                t,
                departure: self.departure,
            })?;
        if t < leg.arrival {
            let ratio = (t - leg.departure).as_seconds_f64() / leg.duration.as_secs_f64();
            let position = leg
                .shape
                .point_at_ratio_from_start(&Haversine, ratio)
                .ok_or(NoWhereabouts::NoPointAlongShape { t })?;
            Ok(Whereabouts::Travelling { position })
        } else {
            Ok(Whereabouts::WaitingAtStation {
                position: leg.destination,
            })
        }
    }

    pub fn sampled(&self, interval: Span) -> Result<Vec<Placed>, NoWhereabouts> {
        let step = interval.time_delta();
        let interpolated = successors(Some(self.departure), |t| Some(*t + step))
            .take_while(|t| *t < self.arrival)
            .map(|t| {
                let (Whereabouts::Travelling { position }
                | Whereabouts::WaitingAtStation { position }) = self.position_at(t)?;
                Ok((t, position, GeometrySource::Interpolated))
            })
            .collect::<Result<Vec<_>, NoWhereabouts>>()?;
        let stops = self.legs.iter().flat_map(|leg| {
            [(leg.departure, leg.origin), (leg.arrival, leg.destination)]
                .map(|(t, position)| (t, position, GeometrySource::Stop))
        });

        let by_instant: BTreeMap<DateTime<Utc>, Placed> = interpolated
            .into_iter()
            .chain(stops)
            .map(|(t, position, geometry_source)| {
                (
                    t,
                    Placed {
                        t,
                        position,
                        geometry_source,
                    },
                )
            })
            .collect();
        Ok(by_instant.into_values().collect())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use geo_types::line_string;

    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_760_000_000 + seconds, 0).expect("an instant")
    }

    fn leg(departure: i64, arrival: i64, shape: LineString<f64>) -> Leg {
        Leg::new(at(departure), at(arrival), shape).expect("a leg")
    }

    fn normalised(legs: &[Leg]) -> NormalisedLegs {
        NormalisedLegs::new(legs).expect("normalised legs")
    }

    fn every_10s() -> Span {
        "10s".parse().expect("a span")
    }

    fn north(latitude: f64) -> LineString<f64> {
        line_string![(x: 8.0, y: 50.0), (x: 8.0, y: latitude)]
    }

    fn near(actual: Point<f64>, expected: (f64, f64)) -> bool {
        (actual.x() - expected.0).abs() < 1e-6 && (actual.y() - expected.1).abs() < 1e-6
    }

    fn sample_at(samples: &[Placed], seconds: i64) -> Placed {
        *samples
            .iter()
            .find(|sample| sample.t == at(seconds))
            .unwrap_or_else(|| panic!("no sample at {seconds} s in {samples:?}"))
    }

    #[test]
    fn a_leg_cannot_arrive_before_it_departs() {
        assert_eq!(
            Leg::new(at(60), at(0), north(50.1)),
            Err(InvalidLeg::ArrivesBeforeDeparting {
                departure: at(60),
                arrival: at(0),
            })
        );
    }

    #[test]
    fn a_leg_cannot_have_a_shape_of_one_point() {
        assert_eq!(
            Leg::new(at(0), at(60), line_string![(x: 8.0, y: 50.0)]),
            Err(InvalidLeg::TooFewPoints {
                departure: at(0),
                points: 1,
            })
        );
    }

    #[test]
    fn a_leg_can_arrive_as_it_departs() {
        assert!(Leg::new(at(60), at(60), north(50.1)).is_ok());
    }

    #[test]
    fn no_legs_cannot_be_normalised() {
        assert_eq!(NormalisedLegs::new(&[]), Err(InvalidLegs::NoLegs));
    }

    #[test]
    fn a_leg_cannot_depart_before_the_one_before_it_arrives() {
        let onward = line_string![(x: 8.0, y: 50.1), (x: 8.1, y: 50.1)];

        assert_eq!(
            NormalisedLegs::new(&[leg(0, 60, north(50.1)), leg(30, 90, onward)]),
            Err(InvalidLegs::Overlapping {
                arrival: at(60),
                next_departure: at(30),
            })
        );
    }

    #[test]
    fn a_leg_can_depart_as_the_one_before_it_arrives() {
        let onward = line_string![(x: 8.0, y: 50.1), (x: 8.1, y: 50.1)];

        assert!(NormalisedLegs::new(&[leg(0, 60, north(50.1)), leg(60, 120, onward)]).is_ok());
    }

    #[test]
    fn no_whereabouts_are_known_before_the_first_leg_departs() {
        assert_eq!(
            normalised(&[leg(60, 120, north(50.1))]).position_at(at(0)),
            Err(NoWhereabouts::BeforeAllLegs {
                t: at(0),
                departure: at(60),
            })
        );
    }

    #[test]
    fn no_whereabouts_are_known_after_the_last_leg_arrives() {
        assert_eq!(
            normalised(&[leg(60, 120, north(50.1))]).position_at(at(180)),
            Err(NoWhereabouts::AfterAllLegs {
                t: at(180),
                arrival: at(120),
            })
        );
    }

    #[test]
    fn during_a_leg_the_train_is_travelling_along_its_shape() {
        let whereabouts = normalised(&[leg(0, 60, north(50.1))]).position_at(at(30));

        assert!(
            matches!(whereabouts, Ok(Whereabouts::Travelling { position }) if near(position, (8.0, 50.05))),
            "{whereabouts:?}"
        );
    }

    #[test]
    fn between_legs_the_train_is_waiting_at_the_station() {
        let onward = line_string![(x: 8.0, y: 50.1), (x: 8.1, y: 50.1)];

        let whereabouts =
            normalised(&[leg(0, 60, north(50.1)), leg(120, 180, onward)]).position_at(at(90));

        assert!(
            matches!(whereabouts, Ok(Whereabouts::WaitingAtStation { position }) if near(position, (8.0, 50.1))),
            "{whereabouts:?}"
        );
    }

    #[test]
    fn on_arriving_the_train_is_waiting_at_the_station() {
        let whereabouts = normalised(&[leg(0, 60, north(50.1))]).position_at(at(60));

        assert!(
            matches!(whereabouts, Ok(Whereabouts::WaitingAtStation { position }) if near(position, (8.0, 50.1))),
            "{whereabouts:?}"
        );
    }

    #[test]
    fn a_train_is_at_its_first_stop_as_it_departs() {
        let samples = normalised(&[leg(0, 60, north(50.1))])
            .sampled(every_10s())
            .expect("samples");

        let first = samples.first().expect("a sample");
        assert_eq!(first.t, at(0));
        assert!(near(first.position, (8.0, 50.0)), "{first:?}");
        assert_eq!(first.geometry_source, GeometrySource::Stop);
    }

    #[test]
    fn a_train_is_at_its_last_stop_as_it_arrives() {
        let samples = normalised(&[leg(0, 60, north(50.1))])
            .sampled(every_10s())
            .expect("samples");

        let last = samples.last().expect("a sample");
        assert_eq!(last.t, at(60));
        assert!(near(last.position, (8.0, 50.1)), "{last:?}");
        assert_eq!(last.geometry_source, GeometrySource::Stop);
    }

    #[test]
    fn a_sample_is_taken_every_interval() {
        let samples = normalised(&[leg(0, 120, north(50.1))])
            .sampled(every_10s())
            .expect("samples");

        let gaps: Vec<TimeDelta> = samples
            .windows(2)
            .map(|pair| pair[1].t - pair[0].t)
            .collect();
        assert_eq!(gaps, vec![TimeDelta::seconds(10); 12]);
    }

    #[test]
    fn a_sample_between_stops_is_interpolated() {
        let samples = normalised(&[leg(0, 60, north(50.1))])
            .sampled(every_10s())
            .expect("samples");

        assert!(
            samples[1..samples.len() - 1]
                .iter()
                .all(|sample| sample.geometry_source == GeometrySource::Interpolated),
            "{samples:?}"
        );
    }

    #[test]
    fn halfway_through_a_leg_the_train_is_halfway_along_its_shape() {
        let bent = line_string![(x: 8.0, y: 50.0), (x: 8.0, y: 50.01), (x: 8.0, y: 50.1)];

        let samples = normalised(&[leg(0, 60, bent)])
            .sampled(every_10s())
            .expect("samples");

        assert!(
            near(sample_at(&samples, 30).position, (8.0, 50.05)),
            "{samples:?}"
        );
    }

    #[test]
    fn between_legs_the_train_waits_at_the_stop() {
        let onward = line_string![(x: 8.0, y: 50.1), (x: 8.1, y: 50.1)];

        let samples = normalised(&[leg(0, 60, north(50.1)), leg(120, 180, onward)])
            .sampled(every_10s())
            .expect("samples");

        let waiting = sample_at(&samples, 90);
        assert!(near(waiting.position, (8.0, 50.1)), "{waiting:?}");
        assert_eq!(waiting.geometry_source, GeometrySource::Interpolated);
    }

    #[test]
    fn an_arrival_and_a_departure_at_one_instant_are_one_stop_sample() {
        let onward = line_string![(x: 8.0, y: 50.1), (x: 8.1, y: 50.1)];

        let samples = normalised(&[leg(0, 60, north(50.1)), leg(60, 120, onward)])
            .sampled(every_10s())
            .expect("samples");

        let at_the_stop: Vec<&Placed> =
            samples.iter().filter(|sample| sample.t == at(60)).collect();
        assert_eq!(at_the_stop.len(), 1, "{samples:?}");
        assert_eq!(at_the_stop[0].geometry_source, GeometrySource::Stop);
    }

    #[test]
    fn a_stop_between_two_intervals_is_kept() {
        let samples = normalised(&[leg(0, 65, north(50.1))])
            .sampled(every_10s())
            .expect("samples");

        let instants: Vec<DateTime<Utc>> = samples.iter().map(|sample| sample.t).collect();
        assert_eq!(instants, [0, 10, 20, 30, 40, 50, 60, 65].map(at));
        assert_eq!(
            sample_at(&samples, 65).geometry_source,
            GeometrySource::Stop
        );
    }

    #[test]
    fn legs_in_any_order_are_sampled_from_the_first_departure() {
        let onward = line_string![(x: 8.0, y: 50.1), (x: 8.1, y: 50.1)];

        let samples = normalised(&[leg(120, 180, onward), leg(0, 60, north(50.1))])
            .sampled(every_10s())
            .expect("samples");

        let instants: Vec<DateTime<Utc>> = samples.iter().map(|sample| sample.t).collect();
        assert_eq!(
            instants,
            (0..=18).map(|step| at(step * 10)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn legs_in_any_order_wait_where_the_last_arrived() {
        let onward = line_string![(x: 8.0, y: 50.1), (x: 8.1, y: 50.1)];
        let beyond = line_string![(x: 8.1, y: 50.1), (x: 8.2, y: 50.1)];

        let samples = normalised(&[
            leg(120, 180, onward),
            leg(0, 60, north(50.1)),
            leg(240, 300, beyond),
        ])
        .sampled(every_10s())
        .expect("samples");

        let waiting = sample_at(&samples, 210);
        assert!(near(waiting.position, (8.1, 50.1)), "{waiting:?}");
    }
}
