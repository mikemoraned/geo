use std::num::NonZeroU32;

use domain::{CrossingCompact, Precision, Radius};
use geo::{Distance, Haversine};

use crate::clock::{Clock, OutOfOrder};
use crate::crossings::Crossings;
use crate::crow_flies::metres;
use crate::detect::{DetectPasses, Passes};
use crate::predict::Event;

pub const DEFAULT_PASS_RADIUS: Radius =
    Radius::metres(NonZeroU32::new(250).expect("two hundred and fifty is not zero"));

#[derive(Debug, Clone)]
pub struct OnlineDetector<P: Precision, C: Crossings<P> = Vec<CrossingCompact<P>>> {
    crossings: C,
    pass_metres: P,
    clock: Clock,
    passing: Passes,
}

impl<P: Precision, C: Crossings<P>> OnlineDetector<P, C> {
    pub fn new(crossings: C, pass_radius: Radius) -> Self {
        Self {
            crossings,
            pass_metres: metres(pass_radius),
            clock: Clock::default(),
            passing: Passes::new(),
        }
    }
}

impl<P: Precision, C: Crossings<P>> DetectPasses<P> for OnlineDetector<P, C> {
    fn observe(&mut self, event: Event<P>) -> Result<(), OutOfOrder> {
        self.clock.observe(&event)?;
        if let Event::Sampled(sample) = event {
            self.passing = self
                .crossings
                .all()
                .filter(|crossing| {
                    Haversine.distance(sample.gps.position, crossing.position) <= self.pass_metres
                })
                .map(|crossing| crossing.id)
                .collect();
        }
        Ok(())
    }

    fn passing(&self) -> &Passes {
        &self.passing
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeDelta, Utc};
    use domain::Sample;

    use super::*;

    fn instant(after: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
            + TimeDelta::seconds(after)
    }

    fn crossings<P: Precision>() -> Vec<CrossingCompact<P>> {
        vec![
            CrossingCompact::at(1, 50.01, 0.0).expect("on the globe"),
            CrossingCompact::at(2, 50.02, 0.0).expect("on the globe"),
            CrossingCompact::at(3, 50.03, 0.0).expect("on the globe"),
        ]
    }

    fn detector(pass_radius_metres: f64) -> OnlineDetector<f64> {
        OnlineDetector::new(
            crossings(),
            Radius::new(pass_radius_metres).expect("a radius"),
        )
    }

    fn sampled(latitude: f64, after: i64) -> Event<f64> {
        Event::Sampled(Sample::at(instant(after), latitude, 0.0).expect("on the globe"))
    }

    fn passed_at(detector: &mut OnlineDetector<f64>, event: Event<f64>) -> Vec<u32> {
        detector.observe(event).expect("an event in order");
        detector
            .passing()
            .iter()
            .map(|crossing_compact_id| crossing_compact_id.get())
            .collect()
    }

    #[test]
    fn nothing_is_reported_before_the_first_sample() {
        assert!(detector(500.0).passing().is_empty());
    }

    #[test]
    fn a_crossing_within_the_pass_radius_of_the_sample_is_reported() {
        let mut detector = detector(500.0);

        assert_eq!(passed_at(&mut detector, sampled(50.012, 0)), [1]);
    }

    #[test]
    fn every_crossing_within_the_pass_radius_is_reported() {
        let mut detector = detector(1_500.0);

        assert_eq!(passed_at(&mut detector, sampled(50.015, 0)), [1, 2]);
    }

    #[test]
    fn a_crossing_beyond_the_pass_radius_is_not_reported() {
        let mut detector = detector(500.0);

        assert!(passed_at(&mut detector, sampled(50.0, 0)).is_empty());
    }

    #[test]
    fn a_crossing_is_reported_for_as_long_as_it_is_within_the_pass_radius() {
        let mut detector = detector(500.0);

        let reported = [(50.008, 0), (50.01, 10), (50.012, 20), (50.02, 30)]
            .map(|(latitude, after)| passed_at(&mut detector, sampled(latitude, after)));

        assert_eq!(reported, [vec![1], vec![1], vec![1], vec![2]]);
    }

    #[test]
    fn time_passing_without_a_sample_keeps_what_the_latest_sample_reported() {
        let mut detector = detector(500.0);
        passed_at(&mut detector, sampled(50.01, 0));

        assert_eq!(passed_at(&mut detector, Event::Elapsed(instant(10))), [1]);
    }

    #[test]
    fn a_sample_behind_the_last_is_refused_and_leaves_the_report_alone() {
        let mut detector = detector(500.0);
        passed_at(&mut detector, sampled(50.01, 10));

        let refused = detector.observe(sampled(50.02, 0));

        assert_eq!(
            refused,
            Err(OutOfOrder {
                now: instant(10),
                at: instant(0)
            })
        );
        assert_eq!(passed_at(&mut detector, Event::Elapsed(instant(10))), [1]);
    }

    #[test]
    fn the_detector_runs_at_the_precision_the_device_uses() {
        let mut detector: OnlineDetector<f32> =
            OnlineDetector::new(crossings(), Radius::new(500.0).expect("a radius"));
        let sample = Sample::<f32>::at(instant(0), 50.01, 0.0).expect("on the globe");

        detector
            .observe(Event::Sampled(sample))
            .expect("an event in order");

        assert_eq!(
            detector
                .passing()
                .iter()
                .map(|crossing_compact_id| crossing_compact_id.get())
                .collect::<Vec<_>>(),
            [1]
        );
    }
}
