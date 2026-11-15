use geo::{Bearing, Distance, Haversine};

use domain::{CrossingCompact, HalfWidth, Precision, Sample, Span};

use crate::clock::{Clock, OutOfOrder};
use crate::crossings::Crossings;
use crate::crow_flies::{Watching, arrival, arrives_within, metres};
use crate::predict::{Confidence, Event, Predict, Prediction, Predictions};
use crate::velocity::{LastMove, Velocity, VelocityEstimator};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cone {
    pub half_width: HalfWidth,
    pub horizon: Span,
}

#[derive(Debug, Clone)]
pub struct LightCone<
    P: Precision,
    C: Crossings<P> = Vec<CrossingCompact<P>>,
    V: VelocityEstimator<P> = LastMove<P>,
> {
    crossings: C,
    watching: Watching,
    cone: Cone,
    clock: Clock,
    velocity: V,
    predictions: Predictions<P>,
}

impl<P: Precision, C: Crossings<P>, V: VelocityEstimator<P>> LightCone<P, C, V> {
    pub fn new(crossings: C, watching: Watching, cone: Cone, velocity: V) -> Self {
        Self {
            crossings,
            watching,
            cone,
            clock: Clock::default(),
            velocity,
            predictions: Predictions::new(),
        }
    }

    fn predict(&mut self, sample: Sample<P>) {
        self.velocity.observe(sample);
        let velocity = self.velocity.velocity();
        let from = sample.gps.position;
        let watching_metres: P = metres(self.watching.radius);
        let cone = self.cone;

        self.predictions = self
            .crossings
            .all()
            .filter_map(|crossing| {
                let metres = Haversine.distance(from, crossing.position);
                (metres <= watching_metres).then(|| {
                    let at =
                        velocity.and_then(|velocity| arrival(sample.t, metres, velocity.speed_mps));
                    let inside = velocity.is_some_and(|velocity| {
                        within_half_width(
                            velocity,
                            Haversine.bearing(from, crossing.position),
                            cone.half_width,
                        )
                    }) && arrives_within(sample.t, at, cone.horizon.time_delta());
                    (
                        crossing.id,
                        Prediction {
                            metres,
                            at,
                            confidence: if inside {
                                Confidence::High
                            } else {
                                Confidence::Low
                            },
                        },
                    )
                })
            })
            .collect();
    }
}

fn within_half_width<P: Precision>(
    velocity: Velocity<P>,
    bearing: P,
    half_width: HalfWidth,
) -> bool {
    let turn = (bearing - velocity.heading_degrees).to_f64().map(|turn| {
        let turn = turn.rem_euclid(360.0);
        turn.min(360.0 - turn)
    });
    turn.is_some_and(|turn| turn <= half_width.as_degrees())
}

impl<P: Precision, C: Crossings<P>, V: VelocityEstimator<P>> Predict<P> for LightCone<P, C, V> {
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
    use chrono::{DateTime, TimeDelta, Utc};
    use domain::Radius;

    use super::*;

    const AHEAD_1_KM: u32 = 1;
    const AHEAD_2_KM: u32 = 2;
    const AHEAD_3_KM: u32 = 3;
    const BESIDE: u32 = 4;
    const BEHIND: u32 = 5;
    const FAR_AHEAD: u32 = 6;

    fn instant() -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
    }

    fn crossings() -> Vec<CrossingCompact<f64>> {
        vec![
            CrossingCompact::at(AHEAD_1_KM, 50.01, 0.0).expect("on the globe"),
            CrossingCompact::at(AHEAD_2_KM, 50.02, 0.0).expect("on the globe"),
            CrossingCompact::at(AHEAD_3_KM, 50.03, 0.0).expect("on the globe"),
            CrossingCompact::at(BESIDE, 50.0, 0.01).expect("on the globe"),
            CrossingCompact::at(BEHIND, 49.98, 0.0).expect("on the globe"),
            CrossingCompact::at(FAR_AHEAD, 50.1, 0.0).expect("on the globe"),
        ]
    }

    fn predictor(half_width_degrees: f64) -> LightCone<f64> {
        LightCone::new(
            crossings(),
            Watching {
                radius: Radius::new(5_000.0).expect("a radius"),
            },
            Cone {
                half_width: HalfWidth::new(half_width_degrees).expect("a half-width"),
                horizon: Span::new("5m").expect("a span"),
            },
            LastMove::new(Radius::new(20.0).expect("a radius")),
        )
    }

    fn fix_at(latitude: f64, after: i64) -> Sample<f64> {
        Sample::at(instant() + TimeDelta::seconds(after), latitude, 0.0).expect("on the globe")
    }

    fn observed(mut predictor: LightCone<f64>, samples: &[Sample<f64>]) -> LightCone<f64> {
        for &sample in samples {
            predictor
                .observe(Event::Sampled(sample))
                .expect("an event in order");
        }
        predictor
    }

    fn heading_north_at_10_mps() -> [Sample<f64>; 2] {
        [fix_at(49.99, -111), fix_at(50.0, 0)]
    }

    fn high(predictor: &LightCone<f64>) -> Vec<u32> {
        predictor
            .predictions()
            .iter()
            .filter(|(_, prediction)| prediction.confidence == Confidence::High)
            .map(|(id, _)| id.get())
            .collect()
    }

    fn predicted(predictor: &LightCone<f64>) -> Vec<u32> {
        predictor.predictions().keys().map(|id| id.get()).collect()
    }

    #[test]
    fn every_crossing_within_the_watching_radius_is_predicted() {
        let predictor = observed(predictor(45.0), &heading_north_at_10_mps());

        assert_eq!(
            predicted(&predictor),
            vec![AHEAD_1_KM, AHEAD_2_KM, AHEAD_3_KM, BESIDE, BEHIND]
        );
    }

    #[test]
    fn a_crossing_ahead_that_the_train_reaches_within_the_horizon_has_high_confidence() {
        let predictor = observed(predictor(45.0), &heading_north_at_10_mps());

        assert_eq!(
            high(&predictor),
            vec![AHEAD_1_KM, AHEAD_2_KM],
            "at 10 m/s the crossings ahead arrive in about 111 s, 222 s, and 333 s, against 5 min",
        );
    }

    #[test]
    fn a_crossing_beside_the_heading_has_high_confidence_only_in_a_cone_wide_enough() {
        let narrow = observed(predictor(45.0), &heading_north_at_10_mps());
        let wide = observed(predictor(100.0), &heading_north_at_10_mps());

        assert_eq!(
            (
                high(&narrow).contains(&BESIDE),
                high(&wide).contains(&BESIDE)
            ),
            (false, true)
        );
    }

    #[test]
    fn a_crossing_behind_has_high_confidence_only_in_a_cone_that_turns_all_the_way_round() {
        let wide = observed(predictor(100.0), &heading_north_at_10_mps());
        let whole = observed(predictor(180.0), &heading_north_at_10_mps());

        assert_eq!(
            (
                high(&wide).contains(&BEHIND),
                high(&whole).contains(&BEHIND)
            ),
            (false, true)
        );
    }

    #[test]
    fn a_train_with_no_move_yet_has_no_crossing_with_high_confidence() {
        let predictor = observed(predictor(180.0), &[fix_at(50.0, 0)]);

        assert_eq!(high(&predictor), Vec::<u32>::new());
    }

    #[test]
    fn a_dwelling_train_keeps_the_heading_of_its_last_move() {
        let predictor = observed(
            predictor(45.0),
            &[fix_at(49.99, -111), fix_at(50.0, 0), fix_at(50.0, 111)],
        );

        assert_eq!(
            high(&predictor),
            vec![AHEAD_1_KM],
            "at 5 m/s since its last move the crossings ahead arrive in about 222 s, 444 s, and \
             666 s",
        );
    }

    #[test]
    fn the_receivers_own_speed_is_not_read() {
        let [before, now] = heading_north_at_10_mps();
        let predictor = observed(predictor(45.0), &[before, now.with_speed_mps(Some(1.0))]);

        assert_eq!(high(&predictor), vec![AHEAD_1_KM, AHEAD_2_KM]);
    }

    #[test]
    fn a_sample_out_of_order_is_refused() {
        let mut predictor = observed(predictor(45.0), &[fix_at(50.0, 0)]);

        assert!(
            predictor
                .observe(Event::Sampled(fix_at(50.0, -10)))
                .is_err()
        );
    }
}
