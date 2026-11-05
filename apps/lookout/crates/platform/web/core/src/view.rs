use chrono::{DateTime, Utc};
use domain::CrossingCompact;
use geo_types::{LineString, Point};
use platform_core::lines::Wkb;
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Rail,
    Trace,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub category: Category,
    pub wkb: Wkb,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewModel {
    pub now: Option<DateTime<Utc>>,
    pub crossings: usize,
    pub radius_metres: Float,
    pub here: Option<Here>,
    pub predicted: Vec<Predicted>,
    pub lines: Vec<Line>,
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

    fn draws_lines() -> bool {
        true
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
                .map(|crossings| located(crossings, model.predictions()))
                .unwrap_or_default(),
            lines: drawn(model),
        }
    }
}

fn drawn(model: &Model<Browser>) -> Vec<Line> {
    let rail = model.lines().iter().map(|wkb| Line {
        category: Category::Rail,
        wkb: wkb.clone(),
    });
    let trace: LineString<f64> = model
        .trace()
        .map(|at| (f64::from(at.x()), f64::from(at.y())))
        .collect();
    let traced = (trace.0.len() > 1).then(|| Line {
        category: Category::Trace,
        wkb: Wkb::of_line(&trace).expect("a line writes into a Vec, which cannot fail"),
    });
    rail.chain(traced).collect()
}

fn located(crossings: &impl Crossings<Float>, predictions: &Predictions<Float>) -> Vec<Predicted> {
    let mut located: Vec<Predicted> = crossings
        .all()
        .filter_map(|crossing_compact| {
            let prediction = predictions.get(&crossing_compact.id)?;
            Some(Predicted {
                position: crossing_compact.position,
                metres: prediction.metres,
                at: prediction.at,
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
    use domain::{CrossingCompactId, Gps, Precision, Sample};
    use platform_core::{Effect, Event, Lookout};
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

    #[test]
    fn a_prediction_is_placed_where_its_crossing_is() {
        let placed = located(
            &crossings(),
            &Predictions::from([prediction(1, 2_000.0, None)]),
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
        );

        let metres: Vec<Float> = placed.iter().map(|one| one.metres).collect();
        assert_eq!(metres, vec![1_000.0, 2_000.0, 3_000.0]);
    }

    #[test]
    fn a_prediction_naming_a_crossing_the_set_lacks_is_dropped() {
        let placed = located(
            &crossings(),
            &Predictions::from([prediction(99, 1_000.0, None)]),
        );

        assert!(placed.is_empty());
    }

    #[test]
    fn an_arrival_is_carried_as_the_instant_it_was_predicted_for() {
        let at = instant() + TimeDelta::seconds(84);

        let placed = located(
            &crossings(),
            &Predictions::from([prediction(0, 1_000.0, Some(at))]),
        );

        assert_eq!(placed[0].at, Some(at));
    }

    fn line() -> Wkb {
        Wkb::new(vec![
            1, 2, 0, 0, 0, 2, 0, 0, 0, 246, 40, 92, 143, 194, 117, 43, 64, 133, 235, 81, 184, 30,
            133, 73, 64, 123, 20, 174, 71, 225, 122, 43, 64, 102, 102, 102, 102, 102, 134, 73, 64,
        ])
        .expect("a line in WKB")
    }

    fn a_kilometre_north() -> Gps<f64> {
        Gps::at(51.0403 + 1_000.0 / 111_195.0, 13.7322)
            .expect("on the globe")
            .with_speed_mps(Some(27.8))
    }

    fn located_at_the_station() -> Core<Lookout<Browser>> {
        let core: Core<Lookout<Browser>> = Core::new();
        core.process_event(Event::Crossings(vec![
            CrossingCompact::at(1, 51.0503, 13.7322).expect("on the globe"),
        ]));
        core
    }

    fn categories(view: &ViewModel) -> Vec<Category> {
        view.lines.iter().map(|line| line.category).collect()
    }

    #[test]
    fn the_rail_received_is_drawn_as_rail() {
        let core = located_at_the_station();
        let id = core
            .process_event(Event::Position(Sample::new(instant(), at_the_station())))
            .iter()
            .find_map(|effect| match effect {
                Effect::Lines(request) => Some(request.operation.id),
                _ => None,
            })
            .expect("a request for lines");

        core.process_event(Event::Lines {
            id,
            lines: vec![line()],
        });

        assert_eq!(
            core.view().lines,
            vec![Line {
                category: Category::Rail,
                wkb: line(),
            }]
        );
    }

    #[test]
    fn the_trace_is_one_line_from_the_oldest_fix_to_the_latest() {
        let core = located_at_the_station();
        core.process_event(Event::Position(Sample::new(instant(), at_the_station())));

        core.process_event(Event::Position(Sample::new(
            instant() + TimeDelta::seconds(36),
            a_kilometre_north(),
        )));

        assert_eq!(
            core.view().lines,
            vec![Line {
                category: Category::Trace,
                wkb: Wkb::new(vec![
                    1, 2, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 224, 226, 118, 43, 64, 0, 0, 0, 128, 40,
                    133, 73, 64, 0, 0, 0, 224, 226, 118, 43, 64, 0, 0, 0, 64, 79, 134, 73, 64,
                ])
                .expect("a line in WKB"),
            }]
        );
    }

    #[test]
    fn the_trace_is_drawn_over_the_rail() {
        let core = located_at_the_station();
        let id = core
            .process_event(Event::Position(Sample::new(instant(), at_the_station())))
            .iter()
            .find_map(|effect| match effect {
                Effect::Lines(request) => Some(request.operation.id),
                _ => None,
            })
            .expect("a request for lines");
        core.process_event(Event::Lines {
            id,
            lines: vec![line()],
        });

        core.process_event(Event::Position(Sample::new(
            instant() + TimeDelta::seconds(36),
            a_kilometre_north(),
        )));

        assert_eq!(
            categories(&core.view()),
            vec![Category::Rail, Category::Trace]
        );
    }

    #[test]
    fn a_single_fix_draws_no_trace() {
        let core = located_at_the_station();

        core.process_event(Event::Position(Sample::new(instant(), at_the_station())));

        assert!(core.view().lines.is_empty());
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
