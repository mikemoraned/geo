use std::collections::BTreeMap;
use std::num::NonZeroU32;

use chrono::{DateTime, TimeDelta, Utc};
use geo::{Distance, Haversine};

use domain::{CrossingCompact, CrossingCompactId, Precision, Radius, Sample, Span};

use crate::clock::{Clock, OutOfOrder};
use crate::crossings::Crossings;
use crate::predict::{Confidence, Event, Predict, Prediction, Predictions};

pub const DEFAULT_WATCHING: Watching = Watching {
    radius: Radius::metres(NonZeroU32::new(5_000).expect("five thousand is not zero")),
};

pub const DEFAULT_ALERTING: Alerting = Alerting {
    radius: Radius::metres(NonZeroU32::new(1_000).expect("one thousand is not zero")),
    horizon: Span::minutes(NonZeroU32::new(5).expect("five is not zero")),
    cooldown: None,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Watching {
    pub radius: Radius,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Alerting {
    pub radius: Radius,
    pub horizon: Span,
    pub cooldown: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[error(
    "an alerting radius of {} m reaches beyond the watching radius of {} m",
    .alerting.as_metres(),
    .watching.as_metres()
)]
pub struct AlertingBeyondWatching {
    pub watching: Radius,
    pub alerting: Radius,
}

#[derive(Debug, Clone)]
pub struct CrowFlies<P: Precision, C: Crossings<P> = Vec<CrossingCompact<P>>> {
    crossings: C,
    watching: Watching,
    alerting: Alerting,
    clock: Clock,
    latest: Option<Sample<P>>,
    speed_mps: Option<P>,
    predictions: Predictions<P>,
    alerting_since: BTreeMap<CrossingCompactId, DateTime<Utc>>,
}

impl<P: Precision, C: Crossings<P>> CrowFlies<P, C> {
    pub fn new(
        crossings: C,
        watching: Watching,
        alerting: Alerting,
    ) -> Result<Self, AlertingBeyondWatching> {
        if alerting.radius.as_metres() > watching.radius.as_metres() {
            return Err(AlertingBeyondWatching {
                watching: watching.radius,
                alerting: alerting.radius,
            });
        }
        Ok(Self {
            crossings,
            watching,
            alerting,
            clock: Clock::default(),
            latest: None,
            speed_mps: None,
            predictions: Predictions::new(),
            alerting_since: BTreeMap::new(),
        })
    }

    pub fn now(&self) -> Option<DateTime<Utc>> {
        self.clock.now()
    }

    pub fn latest(&self) -> Option<&Sample<P>> {
        self.latest.as_ref()
    }

    pub fn speed_mps(&self) -> Option<P> {
        self.speed_mps
    }

    pub fn watching_radius_metres(&self) -> P {
        metres(self.watching.radius)
    }

    pub fn crossings(&self) -> &C {
        &self.crossings
    }

    fn predict(&mut self, sample: Sample<P>) {
        let speed = speed_mps(&sample, self.latest.as_ref());
        let from = sample.gps.position;
        let watching_metres: P = metres(self.watching.radius);
        let alerting_metres: P = metres(self.alerting.radius);
        let horizon = self.alerting.horizon.time_delta();
        let cooldown = self.alerting.cooldown.map(Span::time_delta);
        let alerting_since = &mut self.alerting_since;

        self.predictions = self
            .crossings
            .all()
            .filter_map(|crossing| {
                let metres = Haversine.distance(from, crossing.position);
                (metres <= watching_metres).then(|| {
                    let at = speed.and_then(|speed| arrival(sample.t, metres, speed));
                    let alerted = metres <= alerting_metres
                        && arrives_within(sample.t, at, horizon)
                        && !cooled(alerting_since, crossing.id, sample.t, cooldown);
                    (
                        crossing.id,
                        Prediction {
                            metres,
                            at,
                            confidence: if alerted {
                                Confidence::High
                            } else {
                                Confidence::Low
                            },
                        },
                    )
                })
            })
            .collect();
        self.alerting_since
            .retain(|crossing_compact_id, _| self.predictions.contains_key(crossing_compact_id));
        self.speed_mps = speed;
        self.latest = Some(sample);
    }
}

fn cooled(
    alerting_since: &mut BTreeMap<CrossingCompactId, DateTime<Utc>>,
    crossing_compact_id: CrossingCompactId,
    now: DateTime<Utc>,
    cooldown: Option<TimeDelta>,
) -> bool {
    let since = *alerting_since.entry(crossing_compact_id).or_insert(now);
    cooldown.is_some_and(|cooldown| now - since > cooldown)
}

fn metres<P: Precision>(radius: Radius) -> P {
    // Infallible: `from_f64` only declines a value the target cannot represent, and a radius in
    // metres is an ordinary magnitude in any float.
    P::from_f64(radius.as_metres()).expect("a radius fits in any float")
}

fn arrives_within(now: DateTime<Utc>, at: Option<DateTime<Utc>>, horizon: TimeDelta) -> bool {
    at.is_some_and(|at| at - now <= horizon)
}

fn speed_mps<P: Precision>(sample: &Sample<P>, previous: Option<&Sample<P>>) -> Option<P> {
    sample
        .gps
        .speed_mps
        .or_else(|| implied_speed_mps(sample, previous?))
}

fn implied_speed_mps<P: Precision>(sample: &Sample<P>, previous: &Sample<P>) -> Option<P> {
    let seconds = P::from_f64((sample.t - previous.t).as_seconds_f64())?;
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
    fn observe(&mut self, event: Event<P>) -> Result<(), OutOfOrder> {
        self.clock.observe(&event)?;
        if let Event::Sampled(sample) = event {
            self.predict(sample);
        }
        Ok(())
    }

    fn predictions(&self) -> &Predictions<P> {
        &self.predictions
    }
}

#[cfg(test)]
mod tests {
    use num_traits::float::TotalOrder;

    use crate::predict::nearest_first;

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

    fn radius(metres: f64) -> Radius {
        Radius::new(metres).expect("a radius")
    }

    fn watching(radius_metres: f64) -> Watching {
        Watching {
            radius: radius(radius_metres),
        }
    }

    fn alerting(radius_metres: f64, horizon: &str) -> Alerting {
        Alerting {
            radius: radius(radius_metres),
            horizon: Span::new(horizon).expect("a span"),
            cooldown: None,
        }
    }

    fn predictor() -> CrowFlies<f64> {
        CrowFlies::new(crossings(), watching(5_000.0), alerting(5_000.0, "5m"))
            .expect("alerting within what it watches")
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

    fn ids<P: Precision + TotalOrder>(predictor: &CrowFlies<P>) -> Vec<u32> {
        nearest_first(predictor.predictions())
            .into_iter()
            .map(|(crossing_compact_id, _)| crossing_compact_id.get())
            .collect()
    }

    fn nearest<P: Precision + TotalOrder>(predictor: &CrowFlies<P>, rank: usize) -> Prediction<P> {
        *nearest_first(predictor.predictions())[rank].1
    }

    fn confidences<P: Precision + TotalOrder>(predictor: &CrowFlies<P>) -> Vec<(u32, Confidence)> {
        nearest_first(predictor.predictions())
            .into_iter()
            .map(|(crossing_compact_id, prediction)| {
                (crossing_compact_id.get(), prediction.confidence)
            })
            .collect()
    }

    fn cooling_after(cooldown: Option<&str>) -> CrowFlies<f64> {
        CrowFlies::new(
            crossings(),
            watching(2_000.0),
            Alerting {
                cooldown: cooldown.map(|cooldown| Span::new(cooldown).expect("a span")),
                ..alerting(2_000.0, "5m")
            },
        )
        .expect("alerting within what it watches")
    }

    fn first_crossing_after(
        predictor: &mut CrowFlies<f64>,
        latitude: f64,
        after: i64,
        speed_mps: f64,
    ) -> Option<Confidence> {
        predictor
            .observe(Event::Sampled(
                fix_at(latitude, after).with_speed_mps(Some(speed_mps)),
            ))
            .expect("an event in order");
        predictor
            .predictions()
            .get(&CrossingCompactId::from(1))
            .map(|prediction| prediction.confidence)
    }

    #[test]
    fn a_crossing_alerts_for_no_longer_than_the_cooldown() {
        let mut predictor = cooling_after(Some("1m"));

        let confidences =
            [0, 60, 61].map(|after| first_crossing_after(&mut predictor, 50.0, after, 10.0));

        assert_eq!(
            confidences,
            [
                Some(Confidence::High),
                Some(Confidence::High),
                Some(Confidence::Low)
            ]
        );
    }

    #[test]
    fn a_crossing_that_leaves_the_watching_radius_alerts_again() {
        let mut predictor = cooling_after(Some("1m"));

        let confidences = [(50.0, 0), (50.0, 61), (49.9, 62), (50.0, 63)]
            .map(|(latitude, after)| first_crossing_after(&mut predictor, latitude, after, 10.0));

        assert_eq!(
            confidences,
            [
                Some(Confidence::High),
                Some(Confidence::Low),
                None,
                Some(Confidence::High)
            ]
        );
    }

    #[test]
    fn a_crossing_that_falls_quiet_within_the_watching_radius_keeps_its_cooldown() {
        let mut predictor = cooling_after(Some("1m"));

        let confidences = [(0, 10.0), (30, 0.0), (61, 10.0)]
            .map(|(after, speed)| first_crossing_after(&mut predictor, 50.0, after, speed));

        assert_eq!(
            confidences,
            [
                Some(Confidence::High),
                Some(Confidence::Low),
                Some(Confidence::Low)
            ]
        );
    }

    #[test]
    fn without_a_cooldown_a_crossing_alerts_for_as_long_as_it_arrives_within_the_horizon() {
        let mut predictor = cooling_after(None);

        let confidences =
            [0, 600].map(|after| first_crossing_after(&mut predictor, 50.0, after, 10.0));

        assert_eq!(
            confidences,
            [Some(Confidence::High), Some(Confidence::High)]
        );
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
        let mut predictor = CrowFlies::new(crossings(), watching(2_000.0), alerting(2_000.0, "5m"))
            .expect("alerting within what it watches");

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

        assert_near(nearest(&predictor, 0).metres, HUNDREDTH_DEGREE_M);
        assert_near(nearest(&predictor, 2).metres, 3.0 * HUNDREDTH_DEGREE_M);
    }

    #[test]
    fn the_time_is_the_distance_at_the_reported_speed() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix().with_speed_mps(Some(10.0))))
            .expect("an event in order");

        let predicted = nearest(&predictor, 0).at.expect("a time");
        let seconds = (predicted - instant()).as_seconds_f64();
        assert!(
            (seconds - HUNDREDTH_DEGREE_M / 10.0).abs() < 1.0,
            "{seconds}s is not the time to cover {HUNDREDTH_DEGREE_M}m at 10m/s",
        );
    }

    #[test]
    fn a_crossing_arriving_within_the_horizon_is_predicted_with_high_confidence() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix().with_speed_mps(Some(10.0))))
            .expect("an event in order");

        assert_eq!(
            confidences(&predictor),
            vec![
                (1, Confidence::High),
                (2, Confidence::High),
                (3, Confidence::Low)
            ],
            "at 10m/s the crossings arrive in about 111s, 222s, and 334s, against a 5m horizon",
        );
    }

    #[test]
    fn a_shorter_horizon_holds_fewer_crossings_with_high_confidence() {
        let mut predictor = CrowFlies::new(crossings(), watching(5_000.0), alerting(5_000.0, "2m"))
            .expect("alerting within what it watches");

        predictor
            .observe(Event::Sampled(fix().with_speed_mps(Some(10.0))))
            .expect("an event in order");

        assert_eq!(
            confidences(&predictor),
            vec![
                (1, Confidence::High),
                (2, Confidence::Low),
                (3, Confidence::Low)
            ],
        );
    }

    #[test]
    fn a_crossing_beyond_the_alerting_radius_is_predicted_with_low_confidence() {
        let mut predictor = CrowFlies::new(crossings(), watching(5_000.0), alerting(2_000.0, "5m"))
            .expect("alerting within what it watches");

        predictor
            .observe(Event::Sampled(fix().with_speed_mps(Some(10.0))))
            .expect("an event in order");

        assert_eq!(
            confidences(&predictor),
            vec![
                (1, Confidence::High),
                (2, Confidence::Low),
                (3, Confidence::Low)
            ],
            "every crossing arrives within 5m at 10m/s, but only the first lies within 2km",
        );
    }

    #[test]
    fn an_alerting_radius_beyond_the_watching_radius_is_refused() {
        let refused =
            CrowFlies::<f64>::new(crossings(), watching(1_000.0), alerting(2_000.0, "5m"));

        assert_eq!(
            refused.err(),
            Some(AlertingBeyondWatching {
                watching: radius(1_000.0),
                alerting: radius(2_000.0),
            })
        );
    }

    #[test]
    fn an_alerting_radius_equal_to_the_watching_radius_is_accepted() {
        let accepted =
            CrowFlies::<f64>::new(crossings(), watching(1_000.0), alerting(1_000.0, "5m"));

        assert!(accepted.is_ok());
    }

    #[test]
    fn a_crossing_with_no_time_of_arrival_is_predicted_with_low_confidence() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix().with_speed_mps(Some(0.0))))
            .expect("an event in order");

        assert_eq!(
            confidences(&predictor),
            vec![
                (1, Confidence::Low),
                (2, Confidence::Low),
                (3, Confidence::Low)
            ],
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

        let predicted = nearest(&predictor, 0).at.expect("a time");
        let seconds = (predicted - (instant() + TimeDelta::seconds(100))).as_seconds_f64();
        assert!((seconds - 100.0f64).abs() < 1.0, "{seconds}s is not 100s");
    }

    #[test]
    fn the_first_fix_predicts_a_distance_and_no_time() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix()))
            .expect("an event in order");

        assert_near(nearest(&predictor, 0).metres, HUNDREDTH_DEGREE_M);
        assert_eq!(nearest(&predictor, 0).at, None);
    }

    #[test]
    fn a_stationary_fix_predicts_a_distance_and_no_time() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix().with_speed_mps(Some(0.0))))
            .expect("an event in order");

        assert_near(nearest(&predictor, 0).metres, HUNDREDTH_DEGREE_M);
        assert_eq!(nearest(&predictor, 0).at, None);
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

        assert_eq!(nearest(&predictor, 0).at, None);
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
        let predicted = predictor.predictions().clone();

        predictor
            .observe(Event::Elapsed(instant() + TimeDelta::seconds(60)))
            .expect("an event in order");

        assert_eq!(predictor.now(), Some(instant() + TimeDelta::seconds(60)));
        assert_eq!(predictor.predictions(), &predicted, "the same instants");
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
            Err(OutOfOrder {
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
        let predicted = predictor.predictions().clone();

        let refused = predictor.observe(Event::Sampled(fix_at(50.02, 0)));

        assert!(refused.is_err());
        assert_eq!(predictor.predictions(), &predicted);
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
        assert!(nearest(&predictor, 0).at.is_some(), "the speed landed");
    }

    #[test]
    fn the_whole_prediction_runs_at_the_precision_the_device_uses() {
        let mut predictor: CrowFlies<f32> =
            CrowFlies::new(crossings(), watching(5_000.0), alerting(5_000.0, "5m"))
                .expect("alerting within what it watches");
        let fix = Sample::<f32>::at(instant(), 50.0, 0.0)
            .expect("on the globe")
            .with_speed_mps(Some(10.0));

        predictor
            .observe(Event::Sampled(fix))
            .expect("an event in order");

        assert_eq!(ids(&predictor), vec![1, 2, 3]);
        assert_near(f64::from(nearest(&predictor, 0).metres), HUNDREDTH_DEGREE_M);

        let predicted = nearest(&predictor, 0).at.expect("a time");
        let seconds = (predicted - instant()).as_seconds_f64();
        assert!(
            (seconds - HUNDREDTH_DEGREE_M / 10.0).abs() < 1.0,
            "{seconds}s is not the time to cover {HUNDREDTH_DEGREE_M}m at 10m/s",
        );
    }

    #[test]
    fn an_empty_set_of_crossings_predicts_nothing() {
        let mut predictor = CrowFlies::new(Vec::new(), watching(5_000.0), alerting(5_000.0, "5m"))
            .expect("alerting within what it watches");

        predictor
            .observe(Event::Sampled(fix()))
            .expect("an event in order");

        assert!(predictor.predictions().is_empty());
    }
}
