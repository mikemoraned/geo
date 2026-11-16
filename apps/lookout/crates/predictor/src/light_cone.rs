use geo::{Bearing, Haversine};

use domain::{CrossingCompact, HalfWidth, Precision, Sample};

use crate::clock::OutOfOrder;
use crate::crossings::Crossings;
use crate::crow_flies::CrowFlies;
use crate::predict::{Confidence, Event, Predict, Prediction, Predictions};
use crate::velocity::{LastMove, Velocity, VelocityEstimator};

pub const DEFAULT_HALF_WIDTH: HalfWidth =
    HalfWidth::checked(45.0).expect("45 degrees lies within a half-width's range");

#[derive(Debug, Clone)]
pub struct LightCone<
    P: Precision,
    C: Crossings<P> = Vec<CrossingCompact<P>>,
    V: VelocityEstimator<P> = LastMove<P>,
> {
    crow_flies: CrowFlies<P, C, V>,
    half_width: HalfWidth,
    predictions: Predictions<P>,
}

impl<P: Precision, C: Crossings<P>, V: VelocityEstimator<P>> LightCone<P, C, V> {
    pub fn new(crow_flies: CrowFlies<P, C, V>, half_width: HalfWidth) -> Self {
        Self {
            crow_flies,
            half_width,
            predictions: Predictions::new(),
        }
    }

    pub fn crow_flies(&self) -> &CrowFlies<P, C, V> {
        &self.crow_flies
    }

    fn predict(&mut self, sample: Sample<P>) {
        let from = sample.gps.position;
        let velocity = self.crow_flies.velocity();
        let half_width = self.half_width;
        let predicted = self.crow_flies.predictions();
        self.predictions = self
            .crow_flies
            .crossings()
            .all()
            .filter_map(|crossing| {
                predicted.get(&crossing.id).map(|prediction| {
                    let inside = velocity.is_some_and(|velocity| {
                        within_half_width(
                            velocity,
                            Haversine.bearing(from, crossing.position),
                            half_width,
                        )
                    });
                    (
                        crossing.id,
                        Prediction {
                            confidence: if inside {
                                prediction.confidence
                            } else {
                                Confidence::Low
                            },
                            ..*prediction
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
        self.crow_flies.observe(event)?;
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
    use domain::{Radius, Span};
    use proptest::prelude::*;

    use crate::crow_flies::{Alerting, Watching};

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

    fn crow_flies(
        crossings: Vec<CrossingCompact<f64>>,
        alerting_radius_metres: f64,
    ) -> CrowFlies<f64> {
        CrowFlies::new(
            crossings,
            Watching {
                radius: Radius::new(5_000.0).expect("a radius"),
            },
            Alerting {
                radius: Radius::new(alerting_radius_metres).expect("a radius"),
                horizon: Span::new("5m").expect("a span"),
            },
            LastMove::new(Radius::new(20.0).expect("a radius")),
        )
        .expect("alerting within what it watches")
    }

    fn predictor(half_width_degrees: f64) -> LightCone<f64> {
        LightCone::new(
            crow_flies(crossings(), 5_000.0),
            HalfWidth::new(half_width_degrees).expect("a half-width"),
        )
    }

    fn fix_at(latitude: f64, after: i64) -> Sample<f64> {
        Sample::at(instant() + TimeDelta::seconds(after), latitude, 0.0).expect("on the globe")
    }

    fn observed<Pr: Predict<f64>>(mut predictor: Pr, samples: &[Sample<f64>]) -> Pr {
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

    fn high(predictor: &impl Predict<f64>) -> Vec<u32> {
        predictor
            .predictions()
            .iter()
            .filter(|(_, prediction)| prediction.confidence == Confidence::High)
            .map(|(id, _)| id.get())
            .collect()
    }

    fn predicted(predictor: &impl Predict<f64>) -> Vec<u32> {
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
    fn a_crossing_ahead_beyond_the_alerting_radius_has_low_confidence() {
        let predictor = observed(
            LightCone::new(
                crow_flies(crossings(), 1_500.0),
                HalfWidth::new(45.0).expect("a half-width"),
            ),
            &heading_north_at_10_mps(),
        );

        assert_eq!(high(&predictor), vec![AHEAD_1_KM]);
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

    fn scattered_crossings() -> impl Strategy<Value = Vec<CrossingCompact<f64>>> {
        prop::collection::vec((-0.05f64..0.05, -0.05f64..0.05), 1..20).prop_map(|offsets| {
            offsets
                .into_iter()
                .zip(1u32..)
                .map(|((north, east), id)| {
                    CrossingCompact::at(id, 50.0 + north, east).expect("on the globe")
                })
                .collect()
        })
    }

    fn wandering_track() -> impl Strategy<Value = Vec<Sample<f64>>> {
        prop::collection::vec((-0.003f64..0.003, -0.003f64..0.003, 1i64..30), 1..40).prop_map(
            |steps| {
                steps
                    .into_iter()
                    .scan(
                        (50.0, 0.0, 0),
                        |(latitude, longitude, after), (north, east, wait)| {
                            *latitude += north;
                            *longitude += east;
                            *after += wait;
                            Some(
                                Sample::at(
                                    instant() + TimeDelta::seconds(*after),
                                    *latitude,
                                    *longitude,
                                )
                                .expect("on the globe"),
                            )
                        },
                    )
                    .collect()
            },
        )
    }

    fn confidence_lowered_only(
        crow_flies: &Predictions<f64>,
        light_cone: &Predictions<f64>,
    ) -> bool {
        crow_flies.len() == light_cone.len()
            && crow_flies.iter().all(|(id, by_crow_flies)| {
                light_cone.get(id).is_some_and(|by_light_cone| {
                    by_light_cone.metres == by_crow_flies.metres
                        && by_light_cone.at == by_crow_flies.at
                        && (by_light_cone.confidence == by_crow_flies.confidence
                            || by_light_cone.confidence == Confidence::Low)
                })
            })
    }

    proptest! {
        #[test]
        fn a_cone_turning_all_the_way_round_predicts_what_crow_flies_does(
            crossings in scattered_crossings(),
            track in wandering_track(),
        ) {
            let mut crow_flies = crow_flies(crossings.clone(), 2_000.0);
            let mut light_cone = LightCone::new(
                self::crow_flies(crossings, 2_000.0),
                HalfWidth::new(180.0).expect("a half-width"),
            );

            for sample in track {
                crow_flies.observe(Event::Sampled(sample)).expect("an event in order");
                light_cone.observe(Event::Sampled(sample)).expect("an event in order");
                prop_assert_eq!(light_cone.predictions(), crow_flies.predictions());
            }
        }

        #[test]
        fn a_cone_of_any_half_width_only_lowers_crow_flies_confidence(
            crossings in scattered_crossings(),
            track in wandering_track(),
            half_width_degrees in 1.0f64..180.0,
        ) {
            let mut crow_flies = crow_flies(crossings.clone(), 2_000.0);
            let mut light_cone = LightCone::new(
                self::crow_flies(crossings, 2_000.0),
                HalfWidth::new(half_width_degrees).expect("a half-width"),
            );

            for sample in track {
                crow_flies.observe(Event::Sampled(sample)).expect("an event in order");
                light_cone.observe(Event::Sampled(sample)).expect("an event in order");
                prop_assert!(
                    confidence_lowered_only(crow_flies.predictions(), light_cone.predictions())
                );
            }
        }
    }
}
