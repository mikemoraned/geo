//! The crow-flies predictor: distance in a straight line, divided by the speed we are going.
//!
//! This is a baseline, not an answer. It knows nothing of the track, so a curve or a river
//! bend puts a crossing nearer, and sooner, than the rails can reach it. The evaluation
//! measures a better predictor against it.

use chrono::{DateTime, TimeDelta, Utc};
use geo::{Distance, Haversine};

use crate::crossing::{Crossing, CrossingId};
use crate::measure::Measure;
use crate::predict::{Event, ObserveError, Predict, Prediction, Trend, Trending};
use crate::sample::Sample;

/// How far ahead to predict. Wide enough that a train at speed has a minute or two of
/// warning, narrow enough to mean something at walking pace.
pub const DEFAULT_RADIUS_METRES: f64 = 5_000.0;

/// How much a distance has to change between fixes before it counts as changing at all.
///
/// A fix wanders by metres from one second to the next when the satellite geometry is poor.
/// Without a band, the trend of a crossing we are standing still beside would flicker.
const HOLDING_METRES: f64 = 10.0;

/// A predictor that measures in straight lines.
#[derive(Debug, Clone)]
pub struct CrowFlies<T: Measure> {
    crossings: Vec<Crossing<T>>,
    radius_metres: T,
    now: Option<DateTime<Utc>>,
    /// The last fix, kept to derive a speed for a receiver that reports none.
    last: Option<Sample<T>>,
    predictions: Vec<Prediction<T>>,
    /// What was predicted at the fix before, which is what a trend is measured against.
    previous: Vec<Prediction<T>>,
}

impl<T: Measure> CrowFlies<T> {
    /// A predictor over `crossings`, reporting everything within `radius_metres`.
    pub fn new(crossings: Vec<Crossing<T>>, radius_metres: f64) -> Self {
        // Infallible: `from_f64` only declines a value the target cannot represent, and a
        // radius in metres is an ordinary magnitude in any float.
        let radius_metres = T::from_f64(radius_metres).expect("a radius fits in any float");
        Self {
            crossings,
            radius_metres,
            now: None,
            last: None,
            predictions: Vec::new(),
            previous: Vec::new(),
        }
    }

    /// The clock, as the last event left it.
    pub fn now(&self) -> Option<DateTime<Utc>> {
        self.now
    }

    /// Moves the clock to `to`, refusing to wind it back.
    ///
    /// An instant equal to the clock is accepted. One fix arrives as several sentences
    /// bearing the same epoch, each saying more than the last.
    fn advance(&mut self, to: DateTime<Utc>) -> Result<(), ObserveError> {
        match self.now {
            Some(now) if to < now => Err(ObserveError::OutOfOrder { now, at: to }),
            _ => {
                self.now = Some(to);
                Ok(())
            }
        }
    }

    /// Predicts afresh from `sample`, keeping what was predicted before it as the trend to
    /// measure the new answer against.
    fn predict(&mut self, sample: Sample<T>) {
        let speed = speed_mps(&sample, self.last.as_ref());
        let from = sample.position;
        let radius_metres = self.radius_metres;

        let mut predicted: Vec<Prediction<T>> = self
            .crossings
            .iter()
            .filter_map(|crossing| {
                let metres = Haversine.distance(from, crossing.position);
                (metres <= radius_metres).then(|| Prediction {
                    crossing: crossing.id,
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

        self.previous = std::mem::replace(&mut self.predictions, predicted);
        self.last = Some(sample);
    }
}

/// The speed to divide a distance by: what the receiver reported, or failing that what the
/// step from the fix before implies.
///
/// A receiver reporting no speed is ordinary: a phone's geolocation leaves it out, and a
/// stationary NMEA receiver reports no course to go with it. Two fixes say how fast.
///
/// At `f32` the derived speed is less exact, and the slower we go the less exact it gets.
/// `f32` resolves latitude to about 0.42m, so each fix carries that much error, and so does
/// the step between two of them. A train covers 30m in a second, which puts the error near
/// 1%. Walking pace covers 1.4m, which puts it near 30%. At `f64` the error does not arise.
fn speed_mps<T: Measure>(sample: &Sample<T>, previous: Option<&Sample<T>>) -> Option<T> {
    sample
        .speed_mps
        .or_else(|| implied_speed_mps(sample, previous?))
}

fn implied_speed_mps<T: Measure>(sample: &Sample<T>, previous: &Sample<T>) -> Option<T> {
    let seconds = T::from_f64((sample.t - previous.t).num_milliseconds() as f64 / 1_000.0)?;
    (seconds > T::zero()).then(|| Haversine.distance(previous.position, sample.position) / seconds)
}

/// When we cover `metres` at `speed_mps`, having set off at `at`.
///
/// Nothing at a standstill, because we never arrive. Nothing either at a speed so small that
/// the answer falls outside the range an instant can be expressed in.
fn arrival<T: Measure>(at: DateTime<Utc>, metres: T, speed_mps: T) -> Option<DateTime<Utc>> {
    if speed_mps <= T::zero() {
        return None;
    }
    let milliseconds = (metres / speed_mps).to_f64()? * 1_000.0;
    at.checked_add_signed(TimeDelta::try_milliseconds(milliseconds as i64)?)
}

impl<T: Measure> Predict<T> for CrowFlies<T> {
    /// The clock moves before anything else does, so an event out of order is refused with
    /// the predictor untouched rather than half applied.
    fn observe(&mut self, event: Event<T>) -> Result<(), ObserveError> {
        match event {
            Event::Sampled(sample) => {
                self.advance(sample.t)?;
                self.predict(sample);
            }
            Event::Elapsed(t) => self.advance(t)?,
        }
        Ok(())
    }

    fn predictions(&self) -> &[Prediction<T>] {
        &self.predictions
    }
}

impl<T: Measure> Trending for CrowFlies<T> {
    fn trend(&self, crossing: CrossingId) -> Option<Trend> {
        let metres = |predictions: &[Prediction<T>]| {
            predictions
                .iter()
                .find(|prediction| prediction.crossing == crossing)
                .and_then(|prediction| prediction.metres.to_f64())
        };
        let change = metres(&self.predictions)? - metres(&self.previous)?;

        Some(match change {
            change if change < -HOLDING_METRES => Trend::Closing,
            change if change > HOLDING_METRES => Trend::Receding,
            _ => Trend::Holding,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rounded to the metre from an independent haversine over the same mean-radius sphere,
    /// so a distance here is checked against the formula rather than against `geo` itself.
    const DEGREE_OF_LATITUDE_M: f64 = 111_195.0;
    /// A hundredth of a degree of latitude, which is what the fixtures below are apart.
    const HUNDREDTH_DEGREE_M: f64 = DEGREE_OF_LATITUDE_M / 100.0;
    const TOLERANCE_M: f64 = 10.0;

    fn instant() -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
    }

    /// Three crossings due north of 50.0N, a hundredth of a degree apart, so the nearest is
    /// about 1,112m away and the furthest about 3,336m.
    fn crossings<T: Measure>() -> Vec<Crossing<T>> {
        vec![
            Crossing::at(1, 50.01, 0.0).expect("on the globe"),
            Crossing::at(2, 50.02, 0.0).expect("on the globe"),
            Crossing::at(3, 50.03, 0.0).expect("on the globe"),
        ]
    }

    fn predictor() -> CrowFlies<f64> {
        CrowFlies::new(crossings(), DEFAULT_RADIUS_METRES)
    }

    /// A fix at 50.0N 0.0E, `after` seconds past the fixed instant.
    fn fix_at(latitude: f64, after: i64) -> Sample<f64> {
        Sample::at(instant() + TimeDelta::seconds(after), latitude, 0.0).expect("on the globe")
    }

    fn fix() -> Sample<f64> {
        fix_at(50.0, 0)
    }

    fn assert_near(got: f64, want: f64) {
        assert!((got - want).abs() < TOLERANCE_M, "{got} is not near {want}");
    }

    fn ids<T: Measure>(predictor: &CrowFlies<T>) -> Vec<u32> {
        predictor
            .predictions()
            .iter()
            .map(|prediction| prediction.crossing.value())
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

    /// The crow-flies distance, which is the great-circle line between the two and takes no
    /// notice of how the track gets there.
    #[test]
    fn the_distance_is_the_straight_line_to_the_crossing() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix()))
            .expect("an event in order");

        assert_near(predictor.predictions()[0].metres, HUNDREDTH_DEGREE_M);
        assert_near(predictor.predictions()[2].metres, 3.0 * HUNDREDTH_DEGREE_M);
    }

    /// The other half of a prediction: that distance at the speed the receiver reports.
    /// 1,112m at 10m/s is 111 seconds from the instant of the fix.
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

    /// A phone that reports no speed still moves, and the two fixes say how fast: a
    /// hundredth of a degree in a hundred seconds is about 11m/s.
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

    /// The first fix of a session has nothing to derive a speed from, so it says how far but
    /// not when. Inventing a speed to put a time against it would be worse than saying
    /// nothing.
    #[test]
    fn the_first_fix_predicts_a_distance_and_no_time() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix()))
            .expect("an event in order");

        assert_near(predictor.predictions()[0].metres, HUNDREDTH_DEGREE_M);
        assert_eq!(predictor.predictions()[0].at, None);
    }

    /// Standing still, we never arrive, so there is no time to give.
    #[test]
    fn a_stationary_fix_predicts_a_distance_and_no_time() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix().with_speed_mps(Some(0.0))))
            .expect("an event in order");

        assert_near(predictor.predictions()[0].metres, HUNDREDTH_DEGREE_M);
        assert_eq!(predictor.predictions()[0].at, None);
    }

    /// Sitting at a platform: two fixes in the same place, so the derived speed is zero too.
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

    /// A clock only goes forwards. A shell ticking with a time it read before the fix it has
    /// already handed over is told so, rather than having the tick dropped in silence.
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

    /// And a refused event changes nothing at all, so a late sample cannot move a prediction
    /// while failing to move the clock.
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

    /// A fix reaches the predictor as several sentences bearing one epoch, each saying more
    /// than the last. The same instant twice is ordinary, not out of order.
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
    fn a_crossing_has_no_trend_until_there_is_a_fix_to_compare_it_against() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix()))
            .expect("an event in order");

        assert_eq!(predictor.trend(CrossingId::new(1)), None);
    }

    /// Running north from 50.0 to 50.03: the crossing at 50.02 is nearer than it was, and the
    /// one at 50.01 is now further behind us than it was ahead. A trend is the distance
    /// changing, not the crossing being passed. One we have already gone by keeps closing
    /// until we are further from it than we started.
    #[test]
    fn a_crossing_we_are_moving_towards_is_closing_and_one_we_have_left_behind_recedes() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix_at(50.0, 0)))
            .expect("an event in order");
        predictor
            .observe(Event::Sampled(fix_at(50.03, 10)))
            .expect("an event in order");

        assert_eq!(predictor.trend(CrossingId::new(2)), Some(Trend::Closing));
        assert_eq!(predictor.trend(CrossingId::new(1)), Some(Trend::Receding));
    }

    /// A fix that has barely moved is not a trend, it is noise.
    #[test]
    fn a_crossing_we_have_hardly_moved_towards_is_holding() {
        let mut predictor = predictor();

        predictor
            .observe(Event::Sampled(fix_at(50.0, 0)))
            .expect("an event in order");
        predictor
            .observe(Event::Sampled(fix_at(50.00001, 10)))
            .expect("an event in order");

        assert_eq!(predictor.trend(CrossingId::new(1)), Some(Trend::Holding));
    }

    #[test]
    fn a_crossing_that_was_never_predicted_has_no_trend() {
        let mut predictor = predictor();
        predictor
            .observe(Event::Sampled(fix_at(50.0, 0)))
            .expect("an event in order");
        predictor
            .observe(Event::Sampled(fix_at(50.0, 10)))
            .expect("an event in order");

        assert_eq!(predictor.trend(CrossingId::new(99)), None);
    }

    /// The same prediction at the measure the device runs in, which is the point of the
    /// measure being a parameter at all. `f32` is not a lesser answer here: over a kilometre
    /// it resolves to about a tenth of a metre, far finer than the fix being measured.
    #[test]
    fn the_whole_prediction_runs_at_the_measure_the_device_uses() {
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
