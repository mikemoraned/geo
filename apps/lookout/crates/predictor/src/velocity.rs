use geo::{Bearing, Distance, Haversine};

use domain::{Precision, Radius, Sample};

use crate::crow_flies::metres;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Velocity<P: Precision> {
    pub speed_mps: P,
    pub heading_degrees: P,
}

pub trait VelocityEstimator<P: Precision> {
    fn observe(&mut self, sample: Sample<P>);

    fn velocity(&self) -> Option<Velocity<P>>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct LastMove<P: Precision> {
    still_within: Radius,
    previous_anchor: Option<Sample<P>>,
    anchor: Option<Sample<P>>,
    latest: Option<Sample<P>>,
}

impl<P: Precision> LastMove<P> {
    pub fn new(still_within: Radius) -> Self {
        Self {
            still_within,
            previous_anchor: None,
            anchor: None,
            latest: None,
        }
    }

    fn moves_from(&self, anchor: &Sample<P>, sample: &Sample<P>) -> bool {
        Haversine.distance(anchor.gps.position, sample.gps.position)
            > metres::<P>(self.still_within)
    }
}

impl<P: Precision> VelocityEstimator<P> for LastMove<P> {
    fn observe(&mut self, sample: Sample<P>) {
        match self.anchor {
            None => self.anchor = Some(sample),
            Some(anchor) if self.moves_from(&anchor, &sample) => {
                self.previous_anchor = Some(anchor);
                self.anchor = Some(sample);
            }
            Some(_) => {}
        }
        self.latest = Some(sample);
    }

    fn velocity(&self) -> Option<Velocity<P>> {
        let from = self.previous_anchor?;
        let to = self.anchor?;
        let latest = self.latest?;
        let seconds = P::from_f64((latest.t - from.t).as_seconds_f64())?;
        (seconds > P::zero()).then(|| Velocity {
            speed_mps: Haversine.distance(from.gps.position, to.gps.position) / seconds,
            heading_degrees: Haversine.bearing(from.gps.position, to.gps.position),
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeDelta, Utc};

    use super::*;

    const HUNDREDTH_DEGREE_M: f64 = 1_111.95;
    const TEN_METRES_NORTH_DEGREES: f64 = 0.0000899;
    const FIVE_METRES_EAST_DEGREES: f64 = 0.00007;

    fn instant() -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
    }

    fn fix_at(latitude: f64, longitude: f64, after: i64) -> Sample<f64> {
        Sample::at(instant() + TimeDelta::seconds(after), latitude, longitude)
            .expect("on the globe")
    }

    fn estimated(samples: &[Sample<f64>]) -> Option<Velocity<f64>> {
        let mut estimator = LastMove::new(Radius::new(20.0).expect("a radius"));
        for &sample in samples {
            estimator.observe(sample);
        }
        estimator.velocity()
    }

    fn assert_velocity(got: Option<Velocity<f64>>, speed_mps: f64, heading_degrees: f64) {
        let got = got.expect("a velocity");
        assert!(
            (got.speed_mps - speed_mps).abs() < 0.01,
            "{} m/s is not {speed_mps} m/s",
            got.speed_mps
        );
        assert!(
            (got.heading_degrees - heading_degrees).abs() < 0.1,
            "{}° is not {heading_degrees}°",
            got.heading_degrees
        );
    }

    #[test]
    fn nothing_observed_has_no_velocity() {
        assert_eq!(estimated(&[]), None);
    }

    #[test]
    fn one_sample_has_no_velocity() {
        assert_eq!(estimated(&[fix_at(50.0, 0.0, 0)]), None);
    }

    #[test]
    fn two_samples_northward_give_their_speed_and_a_heading_of_north() {
        let velocity = estimated(&[fix_at(50.0, 0.0, 0), fix_at(50.01, 0.0, 100)]);

        assert_velocity(velocity, HUNDREDTH_DEGREE_M / 100.0, 0.0);
    }

    #[test]
    fn two_samples_eastward_give_a_heading_of_east() {
        let velocity = estimated(&[fix_at(0.0, 0.0, 0), fix_at(0.0, 0.01, 100)]);

        assert_velocity(velocity, HUNDREDTH_DEGREE_M / 100.0, 90.0);
    }

    #[test]
    fn a_sample_repeating_the_position_before_it_keeps_the_heading_of_the_last_move() {
        let velocity = estimated(&[
            fix_at(50.0, 0.0, 0),
            fix_at(50.01, 0.0, 100),
            fix_at(50.01, 0.0, 200),
        ]);

        assert_velocity(velocity, HUNDREDTH_DEGREE_M / 200.0, 0.0);
    }

    #[test]
    fn a_later_move_replaces_the_one_before() {
        let velocity = estimated(&[
            fix_at(0.0, 0.0, 0),
            fix_at(0.01, 0.0, 100),
            fix_at(0.01, 0.01, 200),
        ]);

        assert_velocity(velocity, HUNDREDTH_DEGREE_M / 100.0, 90.0);
    }

    #[test]
    fn samples_that_never_move_have_no_velocity() {
        assert_eq!(
            estimated(&[fix_at(50.0, 0.0, 0), fix_at(50.0, 0.0, 100)]),
            None
        );
    }

    #[test]
    fn a_sample_wandering_within_the_radius_of_stillness_keeps_the_heading_of_the_last_move() {
        let velocity = estimated(&[
            fix_at(50.0, 0.0, 0),
            fix_at(50.01, 0.0, 100),
            fix_at(50.01, FIVE_METRES_EAST_DEGREES, 200),
        ]);

        assert_velocity(velocity, HUNDREDTH_DEGREE_M / 200.0, 0.0);
    }

    #[test]
    fn steps_within_the_radius_of_stillness_add_up_to_a_move_from_where_the_train_last_moved() {
        let velocity = estimated(&[
            fix_at(50.0, 0.0, 0),
            fix_at(
                50.0 + TEN_METRES_NORTH_DEGREES,
                FIVE_METRES_EAST_DEGREES,
                10,
            ),
            fix_at(50.0 + 2.0 * TEN_METRES_NORTH_DEGREES, 0.0, 20),
            fix_at(
                50.0 + 3.0 * TEN_METRES_NORTH_DEGREES,
                FIVE_METRES_EAST_DEGREES,
                30,
            ),
        ]);

        assert_velocity(velocity, 1.013, 9.47);
    }
}
