use chrono::{DateTime, TimeDelta, Utc};
use geo::{Distance, Haversine};

use domain::{CrossingCompact, Precision, Sample};

use crate::crossings::Crossings;
use crate::predict::{Event, ObserveError, Predict, Prediction};

pub const DEFAULT_RADIUS_METRES: f64 = 5_000.0;

#[derive(Debug, Clone)]
pub struct CrowFlies<P: Precision, C: Crossings<P> = Vec<CrossingCompact<P>>> {
    crossings: C,
    radius_metres: P,
    now: Option<DateTime<Utc>>,
    latest: Option<Sample<P>>,
    speed_mps: Option<P>,
    predictions: Vec<Prediction<P>>,
}

impl<P: Precision, C: Crossings<P>> CrowFlies<P, C> {
    pub fn new(crossings: C, radius_metres: f64) -> Self {
        // Infallible: `from_f64` only declines a value the target cannot represent, and a
        // radius in metres is an ordinary magnitude in any float.
        let radius_metres = P::from_f64(radius_metres).expect("a radius fits in any float");
        Self {
            crossings,
            radius_metres,
            now: None,
            latest: None,
            speed_mps: None,
            predictions: Vec::new(),
        }
    }

    pub fn now(&self) -> Option<DateTime<Utc>> {
        self.now
    }

    pub fn latest(&self) -> Option<&Sample<P>> {
        self.latest.as_ref()
    }

    pub fn speed_mps(&self) -> Option<P> {
        self.speed_mps
    }

    pub fn radius_metres(&self) -> P {
        self.radius_metres
    }

    pub fn crossings(&self) -> &C {
        &self.crossings
    }

    fn advance(&mut self, to: DateTime<Utc>) -> Result<(), ObserveError> {
        match self.now {
            Some(now) if to < now => Err(ObserveError::OutOfOrder { now, at: to }),
            _ => {
                self.now = Some(to);
                Ok(())
            }
        }
    }

    fn reached(&mut self, at: DateTime<Utc>) {
        if self.now.is_none_or(|now| at > now) {
            self.now = Some(at);
        }
    }

    fn predict(&mut self, sample: Sample<P>) {
        let speed = speed_mps(&sample, self.latest.as_ref());
        let from = sample.gps.position;
        let radius_metres = self.radius_metres;

        let mut predicted: Vec<Prediction<P>> = self
            .crossings
            .all()
            .filter_map(|crossing| {
                let metres = Haversine.distance(from, crossing.position);
                (metres <= radius_metres).then(|| Prediction {
                    crossing_compact_id: crossing.id,
                    metres,
                    at: speed.and_then(|speed| arrival(sample.t, metres, speed)),
                })
            })
            .collect();
        // Infallible: a haversine over two checked coordinates is a finite number, so no
        // distance here is NaN and every pair of them orders.
        predicted.sort_by(|one, other| {
            one.metres
                .partial_cmp(&other.metres)
                .expect("a distance is never NaN")
        });

        self.predictions = predicted;
        self.speed_mps = speed;
        self.latest = Some(sample);
    }
}

fn speed_mps<P: Precision>(sample: &Sample<P>, previous: Option<&Sample<P>>) -> Option<P> {
    sample
        .gps
        .speed_mps
        .or_else(|| implied_speed_mps(sample, previous?))
}

fn implied_speed_mps<P: Precision>(sample: &Sample<P>, previous: &Sample<P>) -> Option<P> {
    let seconds = P::from_f64((sample.t - previous.t).num_milliseconds() as f64 / 1_000.0)?;
    (seconds > P::zero())
        .then(|| Haversine.distance(previous.gps.position, sample.gps.position) / seconds)
}

fn arrival<P: Precision>(at: DateTime<Utc>, metres: P, speed_mps: P) -> Option<DateTime<Utc>> {
    if speed_mps <= P::zero() {
        return None;
    }
    let milliseconds = (metres / speed_mps).to_f64()? * 1_000.0;
    at.checked_add_signed(TimeDelta::try_milliseconds(milliseconds as i64)?)
}

impl<P: Precision, C: Crossings<P>> Predict<P> for CrowFlies<P, C> {
    fn observe(&mut self, event: Event<P>) -> Result<(), ObserveError> {
        match event {
            Event::Sampled(sample) => {
                if let Some(latest) = &self.latest
                    && sample.t < latest.t
                {
                    return Err(ObserveError::OutOfOrder {
                        now: latest.t,
                        at: sample.t,
                    });
                }
                self.reached(sample.t);
                self.predict(sample);
            }
            Event::Elapsed(t) => self.advance(t)?,
        }
        Ok(())
    }

    fn predictions(&self) -> &[Prediction<P>] {
        &self.predictions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEGREE_OF_LATITUDE_M: f64 = 111_195.0;
    const HUNDREDTH_DEGREE_M: f64 = DEGREE_OF_LATITUDE_M / 100.0;
    const TOLERANCE_M: f64 = 10.0;

    fn instant() -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
    }

    fn crossings<P: Precision>() -> Vec<CrossingCompact<P>> {
        vec![
            CrossingCompact::at(1, 50.01, 0.0).expect("on the globe"),
            CrossingCompact::at(2, 50.02, 0.0).expect("on the globe"),
            CrossingCompact::at(3, 50.03, 0.0).expect("on the globe"),
        ]
    }

    fn predictor() -> CrowFlies<f64> {
        CrowFlies::new(crossings(), DEFAULT_RADIUS_METRES)
    }

    fn fix_at(latitude: f64, after: i64) -> Sample<f64> {
        Sample::at(instant() + TimeDelta::seconds(after), latitude, 0.0).expect("on the globe")
    }

    fn fix() -> Sample<f64> {
        fix_at(50.0, 0)
    }

    fn assert_near(got: f64, want: f64) {
        assert!((got - want).abs() < TOLERANCE_M, "{got} is not near {want}");
    }

    fn ids<P: Precision>(predictor: &CrowFlies<P>) -> Vec<u32> {
        predictor
            .predictions()
            .iter()
            .map(|prediction| prediction.crossing_compact_id.get())
            .collect()
    }

    #[test]
    fn nothing_is_predicted_before_a_fix() {
        assert!(predictor().predictions().is_empty());
    }

    #[test]
    fn a_fix_predicts_every_crossing_inside_the_radius_nearest_first() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix()))
            .expect("an event in order");

        assert_eq!(ids(&predictor), vec![1, 2, 3]);
    }

    #[test]
    fn a_crossing_outside_the_radius_is_not_predicted() {
        let mut predictor = CrowFlies::new(crossings(), 2_000.0);

        predictor
            .observe(Event::Sampled(fix()))
            .expect("an event in order");

        assert_eq!(ids(&predictor), vec![1], "only the one inside 2km");
    }

    #[test]
    fn the_distance_is_the_straight_line_to_the_crossing() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix()))
            .expect("an event in order");

        assert_near(predictor.predictions()[0].metres, HUNDREDTH_DEGREE_M);
        assert_near(predictor.predictions()[2].metres, 3.0 * HUNDREDTH_DEGREE_M);
    }

    #[test]
    fn the_time_is_the_distance_at_the_reported_speed() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix().with_speed_mps(Some(10.0))))
            .expect("an event in order");

        let predicted = predictor.predictions()[0].at.expect("a time");
        let seconds = (predicted - instant()).num_milliseconds() as f64 / 1_000.0;
        assert!(
            (seconds - HUNDREDTH_DEGREE_M / 10.0).abs() < 1.0,
            "{seconds}s is not the time to cover {HUNDREDTH_DEGREE_M}m at 10m/s",
        );
    }

    #[test]
    fn nothing_is_said_about_speed_before_a_fix_has_been_predicted_from() {
        let predictor = predictor();

        assert_eq!(predictor.speed_mps(), None);
    }

    #[test]
    fn the_speed_it_predicted_at_is_the_one_the_receiver_reported() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix().with_speed_mps(Some(10.0))))
            .expect("an event in order");

        assert_eq!(predictor.speed_mps(), Some(10.0));
    }

    #[test]
    fn the_speed_it_predicted_at_is_the_derived_one_where_none_was_reported() {
        let mut predictor = predictor();
        predictor
            .observe(Event::Sampled(fix_at(49.99, 0)))
            .expect("an event in order");

        predictor
            .observe(Event::Sampled(fix_at(50.0, 100)))
            .expect("an event in order");

        let derived = predictor.speed_mps().expect("a derived speed");
        assert!(
            (derived - HUNDREDTH_DEGREE_M / 100.0).abs() < TOLERANCE_M,
            "{derived}m/s is not a hundredth of a degree in a hundred seconds",
        );
    }

    #[test]
    fn a_fix_behind_the_clock_but_ahead_of_the_last_fix_is_taken() {
        let mut predictor = predictor();
        predictor
            .observe(Event::Sampled(fix_at(49.99, 0)))
            .expect("an event in order");
        predictor
            .observe(Event::Elapsed(instant() + TimeDelta::seconds(60)))
            .expect("a time signal");

        predictor
            .observe(Event::Sampled(fix_at(50.0, 10)))
            .expect("a fix newer than the last one");

        assert_eq!(predictor.latest().expect("a fix").latitude(), 50.0);
        assert_eq!(
            predictor.now(),
            Some(instant() + TimeDelta::seconds(60)),
            "the clock stays where the signal put it rather than winding back to the fix"
        );
    }

    #[test]
    fn a_fix_behind_the_last_fix_is_refused() {
        let mut predictor = predictor();
        predictor
            .observe(Event::Sampled(fix_at(50.0, 30)))
            .expect("an event in order");

        let refused = predictor.observe(Event::Sampled(fix_at(49.99, 10)));

        assert!(refused.is_err());
        assert_eq!(predictor.latest().expect("a fix").latitude(), 50.0);
    }

    #[test]
    fn the_crossings_it_predicts_against_are_the_ones_it_was_given() {
        let predictor = predictor();

        assert_eq!(predictor.crossings().len(), crossings::<f64>().len());
    }

    #[test]
    fn a_speed_the_receiver_does_not_report_is_derived_from_the_fix_before() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix_at(49.99, 0)))
            .expect("an event in order");
        predictor
            .observe(Event::Sampled(fix_at(50.0, 100)))
            .expect("an event in order");

        let predicted = predictor.predictions()[0].at.expect("a time");
        let seconds =
            (predicted - (instant() + TimeDelta::seconds(100))).num_milliseconds() as f64 / 1_000.0;
        assert!((seconds - 100.0f64).abs() < 1.0, "{seconds}s is not 100s");
    }

    #[test]
    fn the_first_fix_predicts_a_distance_and_no_time() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix()))
            .expect("an event in order");

        assert_near(predictor.predictions()[0].metres, HUNDREDTH_DEGREE_M);
        assert_eq!(predictor.predictions()[0].at, None);
    }

    #[test]
    fn a_stationary_fix_predicts_a_distance_and_no_time() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix().with_speed_mps(Some(0.0))))
            .expect("an event in order");

        assert_near(predictor.predictions()[0].metres, HUNDREDTH_DEGREE_M);
        assert_eq!(predictor.predictions()[0].at, None);
    }

    #[test]
    fn a_fix_that_has_not_moved_predicts_no_time() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix_at(50.0, 0)))
            .expect("an event in order");
        predictor
            .observe(Event::Sampled(fix_at(50.0, 10)))
            .expect("an event in order");

        assert_eq!(predictor.predictions()[0].at, None);
    }

    #[test]
    fn a_sample_advances_the_clock_to_its_own_timestamp() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix_at(50.0, 30)))
            .expect("an event in order");

        assert_eq!(predictor.now(), Some(instant() + TimeDelta::seconds(30)));
    }

    #[test]
    fn time_passing_advances_the_clock_and_leaves_the_prediction_alone() {
        let mut predictor = predictor();
        predictor
            .observe(Event::Sampled(fix().with_speed_mps(Some(10.0))))
            .expect("an event in order");
        let predicted = predictor.predictions().to_vec();

        predictor
            .observe(Event::Elapsed(instant() + TimeDelta::seconds(60)))
            .expect("an event in order");

        assert_eq!(predictor.now(), Some(instant() + TimeDelta::seconds(60)));
        assert_eq!(predictor.predictions(), predicted, "the same instants");
    }

    #[test]
    fn a_time_behind_the_clock_is_refused() {
        let mut predictor = predictor();
        predictor
            .observe(Event::Sampled(fix_at(50.0, 30)))
            .expect("an event in order");

        let refused = predictor.observe(Event::Elapsed(instant()));

        assert_eq!(
            refused,
            Err(ObserveError::OutOfOrder {
                now: instant() + TimeDelta::seconds(30),
                at: instant(),
            })
        );
        assert_eq!(predictor.now(), Some(instant() + TimeDelta::seconds(30)));
    }

    #[test]
    fn a_sample_behind_the_clock_is_refused_and_predicts_nothing() {
        let mut predictor = predictor();
        predictor
            .observe(Event::Sampled(fix_at(50.0, 30)))
            .expect("an event in order");
        let predicted = predictor.predictions().to_vec();

        let refused = predictor.observe(Event::Sampled(fix_at(50.02, 0)));

        assert!(refused.is_err());
        assert_eq!(predictor.predictions(), predicted);
        assert_eq!(predictor.now(), Some(instant() + TimeDelta::seconds(30)));
    }

    #[test]
    fn a_second_event_at_the_same_instant_is_accepted() {
        let mut predictor = predictor();
        predictor
            .observe(Event::Sampled(fix_at(50.0, 30)))
            .expect("an event in order");

        let again = predictor.observe(Event::Sampled(fix_at(50.0, 30).with_speed_mps(Some(10.0))));

        assert!(again.is_ok());
        assert!(predictor.predictions()[0].at.is_some(), "the speed landed");
    }

    #[test]
    fn the_whole_prediction_runs_at_the_precision_the_device_uses() {
        let mut predictor: CrowFlies<f32> = CrowFlies::new(crossings(), DEFAULT_RADIUS_METRES);
        let fix = Sample::<f32>::at(instant(), 50.0, 0.0)
            .expect("on the globe")
            .with_speed_mps(Some(10.0));

        predictor
            .observe(Event::Sampled(fix))
            .expect("an event in order");

        assert_eq!(ids(&predictor), vec![1, 2, 3]);
        assert_near(
            f64::from(predictor.predictions()[0].metres),
            HUNDREDTH_DEGREE_M,
        );

        let predicted = predictor.predictions()[0].at.expect("a time");
        let seconds = (predicted - instant()).num_milliseconds() as f64 / 1_000.0;
        assert!(
            (seconds - HUNDREDTH_DEGREE_M / 10.0).abs() < 1.0,
            "{seconds}s is not the time to cover {HUNDREDTH_DEGREE_M}m at 10m/s",
        );
    }

    #[test]
    fn an_empty_set_of_crossings_predicts_nothing() {
        let mut predictor = CrowFlies::new(Vec::new(), DEFAULT_RADIUS_METRES);

        predictor
            .observe(Event::Sampled(fix()))
            .expect("an event in order");

        assert!(predictor.predictions().is_empty());
    }
}
