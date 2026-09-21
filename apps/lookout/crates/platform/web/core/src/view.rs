//! What a browser draws: where we are, how fast, and what is about to be crossed.

use chrono::{DateTime, Utc};
use domain::CrossingCompact;
use geo_types::Point;
use platform_core::{Float, Model, Shell};
use predictor::{Crossings, Prediction};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Here {
    pub position: Point<Float>,
    /// The speed the arrivals below were worked out at.
    pub speed_mps: Option<Float>,
}

/// One crossing we expect to reach.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Predicted {
    pub position: Point<Float>,
    /// Straight-line distance from here. Crow-flies, so a bend in the track puts a
    /// crossing nearer than the rails do.
    pub metres: Float,
    /// When we reach it, absent where there is no speed to divide by. An instant rather than
    /// a countdown, so it stays true while the clock advances between fixes.
    pub at: Option<DateTime<Utc>>,
}

/// Everything the page draws.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewModel {
    /// The time the core is working to, which a countdown is subtracted from.
    pub now: Option<DateTime<Utc>>,
    /// How many crossings are being predicted against. Zero until a set has arrived.
    pub crossings: usize,
    /// How far out the predictions reach. What a drawing puts at its edge.
    pub radius_metres: Float,
    /// Absent until a position has arrived.
    pub here: Option<Here>,
    /// Nearest first, and never more than the radius holds.
    pub predicted: Vec<Predicted>,
}

/// A browser as a shell.
#[derive(Debug, Default, Clone, Copy)]
pub struct Browser;

impl Shell for Browser {
    type ViewModel = ViewModel;
    type Crossings = Vec<CrossingCompact<Float>>;

    /// Nothing: the core exists from the moment the module loads, and the points arrive over
    /// the network after it.
    fn carried() -> Option<Self::Crossings> {
        None
    }

    /// A point the globe has no room for is dropped rather than refusing the whole set: one
    /// bad row should not cost a page every crossing near it.
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
                position: fix.position,
                speed_mps: model.speed_mps(),
            }),
            predicted: model
                .crossings()
                .map(|crossings| located(crossings, model.predictions()))
                .unwrap_or_default(),
        }
    }
}

/// Each prediction with the crossing's position beside it.
///
/// A prediction names a crossing by id, and the set it was predicted against is the only place
/// that id means anything, so the set is read once here rather than per prediction. A crossing
/// the set no longer holds is dropped: the canvas draws a position, and there is none for it.
fn located(crossings: &impl Crossings<Float>, predictions: &[Prediction<Float>]) -> Vec<Predicted> {
    let mut located: Vec<Predicted> = crossings
        .all()
        .filter_map(|crossing| {
            let prediction = predictions
                .iter()
                .find(|prediction| prediction.crossing == crossing.id)?;
            Some(Predicted {
                position: crossing.position,
                metres: prediction.metres,
                at: prediction.at,
            })
        })
        .collect();
    // The set is in whatever order it was packed in; the view is nearest first, as the
    // predictions are.
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
    use domain::{CrossingCompactId, Gps};
    use platform_core::{Event, Lookout};

    use super::*;

    fn instant() -> DateTime<Utc> {
        DateTime::from_timestamp(1_785_098_609, 0).expect("an instant")
    }

    /// Three crossings due north of the fix, a hundredth of a degree apart, so the nearest is
    /// about 1,112m away. Packed out of order, so what sorts the view is the view.
    fn crossings() -> Vec<CrossingCompact<Float>> {
        vec![
            CrossingCompact::at(2, 51.06, 13.7322).expect("on the globe"),
            CrossingCompact::at(0, 51.04, 13.7322).expect("on the globe"),
            CrossingCompact::at(1, 51.05, 13.7322).expect("on the globe"),
        ]
    }

    fn prediction(id: u32, metres: Float, at: Option<DateTime<Utc>>) -> Prediction<Float> {
        Prediction {
            crossing: CrossingCompactId::new(id),
            metres,
            at,
        }
    }

    #[test]
    fn a_prediction_is_placed_where_its_crossing_is() {
        let placed = located(&crossings(), &[prediction(1, 2_000.0, None)]);

        assert_eq!(placed.len(), 1);
        assert!((placed[0].position.y() - 51.05).abs() < 1e-4);
        assert!((placed[0].position.x() - 13.7322).abs() < 1e-4);
        assert_eq!(placed[0].metres, 2_000.0);
    }

    #[test]
    fn the_nearest_is_first_however_the_set_was_packed() {
        let placed = located(
            &crossings(),
            &[
                prediction(0, 1_000.0, None),
                prediction(2, 3_000.0, None),
                prediction(1, 2_000.0, None),
            ],
        );

        let metres: Vec<Float> = placed.iter().map(|one| one.metres).collect();
        assert_eq!(metres, vec![1_000.0, 2_000.0, 3_000.0]);
    }

    /// The canvas draws a position, so a prediction the set cannot place is not in the view.
    #[test]
    fn a_prediction_naming_a_crossing_the_set_lacks_is_dropped() {
        let placed = located(&crossings(), &[prediction(99, 1_000.0, None)]);

        assert!(placed.is_empty());
    }

    #[test]
    fn an_arrival_is_carried_as_the_instant_it_was_predicted_for() {
        let at = instant() + TimeDelta::seconds(84);

        let placed = located(&crossings(), &[prediction(0, 1_000.0, Some(at))]);

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
        core.process_event(Event::Crossings(vec![
            CrossingCompact::at(1, 51.0503, 13.7322).expect("on the globe"),
        ]));

        core.process_event(Event::Position {
            t: instant(),
            gps: Gps {
                latitude: 51.0403,
                longitude: 13.7322,
                altitude_metres: None,
                accuracy_metres: 5.0,
                speed_mps: Some(27.8),
                heading_degrees: None,
            },
        });

        let here = core.view().here.expect("a fix");
        assert!((here.position.y() - 51.0403).abs() < 1e-4);
        assert!((here.position.x() - 13.7322).abs() < 1e-4);
        assert_eq!(here.speed_mps, Some(27.8));
        assert_eq!(core.view().predicted.len(), 1);
    }

    /// The page fetches its crossings and the browser reports a position, and the two race.
    /// Until the set lands there is nothing to measure a fix against, so the view stays empty.
    #[test]
    fn nothing_is_shown_from_a_fix_that_beat_the_crossings() {
        let core: Core<Lookout<Browser>> = Core::new();

        core.process_event(Event::Position {
            t: instant(),
            gps: Gps {
                latitude: 51.0403,
                longitude: 13.7322,
                altitude_metres: None,
                accuracy_metres: 5.0,
                speed_mps: Some(27.8),
                heading_degrees: None,
            },
        });

        assert_eq!(core.view().here, None);
    }
}
