use chrono::{DateTime, Utc};
use domain::{CrossingCompact, CrossingCompactId, StateKind};
use geo_types::Point;
use platform_core::{Float, Model, Shell};
use predictor::{Crossings, Predictions};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Here {
    pub position: Point<Float>,
    pub speed_mps: Option<Float>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Predicted {
    pub position: Point<Float>,
    pub metres: Float,
    pub at: Option<DateTime<Utc>>,
    pub state: StateKind,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewModel {
    pub now: Option<DateTime<Utc>>,
    pub crossings: usize,
    pub radius_metres: Float,
    pub here: Option<Here>,
    pub predicted: Vec<Predicted>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Browser;

impl Shell for Browser {
    type ViewModel = ViewModel;
    type Crossings = Vec<CrossingCompact<Float>>;

    fn carried() -> Option<Self::Crossings> {
        None
    }

    fn received(points: Vec<CrossingCompact<f64>>) -> Option<Self::Crossings> {
        Some(
            points
                .into_iter()
                .filter_map(|point| {
                    CrossingCompact::at(point.id, point.latitude(), point.longitude()).ok()
                })
                .collect(),
        )
    }

    fn project(model: &Model<Self>) -> ViewModel {
        ViewModel {
            now: model.now(),
            crossings: model.crossings().map_or(0, Vec::len),
            radius_metres: model.radius_metres().unwrap_or_default(),
            here: model.fix().map(|fix| Here {
                position: fix.gps.position,
                speed_mps: model.speed_mps(),
            }),
            predicted: model
                .crossings()
                .map(|crossings| located(crossings, model.predictions(), |id| model.state(id)))
                .unwrap_or_default(),
        }
    }
}

fn located(
    crossings: &impl Crossings<Float>,
    predictions: &Predictions<Float>,
    state: impl Fn(CrossingCompactId) -> StateKind,
) -> Vec<Predicted> {
    let mut located: Vec<Predicted> = crossings
        .all()
        .filter_map(|crossing_compact| {
            let prediction = predictions.get(&crossing_compact.id)?;
            Some(Predicted {
                position: crossing_compact.position,
                metres: prediction.metres,
                at: prediction.at,
                state: state(crossing_compact.id),
            })
        })
        .collect();
    located.sort_by(|one, other| {
        one.metres
            .partial_cmp(&other.metres)
            .expect("a distance is never NaN")
    });
    located
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeDelta, Utc};
    use crux_core::Core;
    use domain::places::DRESDEN_HBF;
    use domain::{Gps, Precision, Sample};
    use platform_core::{Event, Lookout};
    use predictor::{Confidence, Prediction};

    use super::*;

    fn at_the_station() -> Gps<f64> {
        Gps::at(DRESDEN_HBF.y(), DRESDEN_HBF.x())
            .expect("on the globe")
            .with_accuracy_metres(Some(5.0))
            .with_speed_mps(Some(27.8))
    }

    fn instant() -> DateTime<Utc> {
        DateTime::from_timestamp(1_785_098_609, 0).expect("an instant")
    }

    fn north_of_the_station(degrees: f64) -> Point<f64> {
        Point::new(DRESDEN_HBF.x(), DRESDEN_HBF.y() + degrees)
    }

    fn crossing<P: Precision>(id: u32, at: Point<f64>) -> CrossingCompact<P> {
        CrossingCompact::at(id, at.y(), at.x()).expect("on the globe")
    }

    fn crossings() -> Vec<CrossingCompact<Float>> {
        vec![
            crossing(2, north_of_the_station(0.02)),
            crossing(0, north_of_the_station(0.0)),
            crossing(1, north_of_the_station(0.01)),
        ]
    }

    fn prediction(
        id: u32,
        metres: Float,
        at: Option<DateTime<Utc>>,
    ) -> (CrossingCompactId, Prediction<Float>) {
        (
            CrossingCompactId::new(id),
            Prediction {
                metres,
                at,
                confidence: Confidence::Low,
            },
        )
    }

    fn candidates(_: CrossingCompactId) -> StateKind {
        StateKind::CandidatePass
    }

    #[test]
    fn a_prediction_carries_the_state_of_its_crossing() {
        let placed = located(
            &crossings(),
            &Predictions::from([prediction(0, 1_000.0, None), prediction(1, 2_000.0, None)]),
            |id| {
                if id == CrossingCompactId::new(0) {
                    StateKind::ActualPass
                } else {
                    StateKind::AlertedPass
                }
            },
        );

        let states: Vec<StateKind> = placed.iter().map(|one| one.state).collect();
        assert_eq!(states, vec![StateKind::ActualPass, StateKind::AlertedPass]);
    }

    #[test]
    fn each_crossing_shown_carries_the_state_its_fix_gives() {
        let core: Core<Lookout<Browser>> = Core::new();
        core.process_event(Event::Crossings(vec![
            crossing(1, north_of_the_station(0.01)),
            crossing(2, north_of_the_station(0.005)),
            crossing(3, north_of_the_station(0.0)),
        ]));

        core.process_event(Event::Position(Sample::new(instant(), at_the_station())));

        let states: Vec<StateKind> = core.view().predicted.iter().map(|one| one.state).collect();
        assert_eq!(
            states,
            vec![
                StateKind::ActualPass,
                StateKind::AlertedPass,
                StateKind::CandidatePass
            ]
        );
    }

    #[test]
    fn a_prediction_is_placed_where_its_crossing_is() {
        let placed = located(
            &crossings(),
            &Predictions::from([prediction(1, 2_000.0, None)]),
            candidates,
        );

        assert_eq!(placed.len(), 1);
        assert!((placed[0].position.y() - north_of_the_station(0.01).y() as Float).abs() < 1e-4);
        assert!((placed[0].position.x() - DRESDEN_HBF.x() as Float).abs() < 1e-4);
        assert_eq!(placed[0].metres, 2_000.0);
    }

    #[test]
    fn the_nearest_is_first_however_the_set_was_packed() {
        let placed = located(
            &crossings(),
            &Predictions::from([
                prediction(0, 1_000.0, None),
                prediction(2, 3_000.0, None),
                prediction(1, 2_000.0, None),
            ]),
            candidates,
        );

        let metres: Vec<Float> = placed.iter().map(|one| one.metres).collect();
        assert_eq!(metres, vec![1_000.0, 2_000.0, 3_000.0]);
    }

    #[test]
    fn a_prediction_naming_a_crossing_the_set_lacks_is_dropped() {
        let placed = located(
            &crossings(),
            &Predictions::from([prediction(99, 1_000.0, None)]),
            candidates,
        );

        assert!(placed.is_empty());
    }

    #[test]
    fn an_arrival_is_carried_as_the_instant_it_was_predicted_for() {
        let at = instant() + TimeDelta::seconds(84);

        let placed = located(
            &crossings(),
            &Predictions::from([prediction(0, 1_000.0, Some(at))]),
            candidates,
        );

        assert_eq!(placed[0].at, Some(at));
    }

    #[test]
    fn there_is_nothing_to_centre_on_before_a_fix() {
        let core: Core<Lookout<Browser>> = Core::new();

        let view = core.view();

        assert_eq!(view.here, None);
        assert_eq!(view.now, None);
        assert!(view.predicted.is_empty());
    }

    #[test]
    fn a_fix_is_shown_with_the_speed_it_was_predicted_at() {
        let core: Core<Lookout<Browser>> = Core::new();
        core.process_event(Event::Crossings(vec![crossing(
            1,
            north_of_the_station(0.01),
        )]));

        core.process_event(Event::Position(Sample::new(instant(), at_the_station())));

        let here = core.view().here.expect("a fix");
        assert!((here.position.y() - DRESDEN_HBF.y() as Float).abs() < 1e-4);
        assert!((here.position.x() - DRESDEN_HBF.x() as Float).abs() < 1e-4);
        assert_eq!(here.speed_mps, Some(27.8));
        assert_eq!(core.view().predicted.len(), 1);
    }

    #[test]
    fn nothing_is_shown_from_a_fix_that_beat_the_crossings() {
        let core: Core<Lookout<Browser>> = Core::new();

        core.process_event(Event::Position(Sample::new(instant(), at_the_station())));

        assert_eq!(core.view().here, None);
    }
}
