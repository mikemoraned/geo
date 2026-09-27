use core::marker::PhantomData;

use chrono::{DateTime, Utc};
use crux_core::{
    App, Command,
    capability::Operation,
    macros::effect,
    render::{self, RenderOperation},
};
use domain::{CrossingCompact, Sample};
use predictor::{
    Crossings, CrowFlies, DEFAULT_RADIUS_METRES, Event as Observed, Parser, Predict, Sentence,
};
use serde::{Deserialize, Serialize};

use crate::Float;
use crate::battery::{Battery, Charge};

pub trait Shell: Sized + 'static {
    type ViewModel;

    type Crossings: Crossings<Float> + Default;

    fn carried() -> Option<Self::Crossings>;

    fn received(points: Vec<CrossingCompact<f64>>) -> Option<Self::Crossings>;

    fn project(model: &Model<Self>) -> Self::ViewModel;
}

pub struct Model<S: Shell> {
    parser: Parser<Float>,
    battery: Battery,
    predictor: Predicting<S>,
}

enum Predicting<S: Shell> {
    Waiting,
    Ready(CrowFlies<Float, S::Crossings>),
}

impl<S: Shell> Default for Model<S> {
    fn default() -> Self {
        Self {
            parser: Parser::new(),
            battery: Battery::default(),
            predictor: match S::carried() {
                Some(crossings) => {
                    Predicting::Ready(CrowFlies::new(crossings, DEFAULT_RADIUS_METRES))
                }
                None => Predicting::Waiting,
            },
        }
    }
}

impl<S: Shell> Model<S> {
    pub fn now(&self) -> Option<DateTime<Utc>> {
        self.ready().and_then(CrowFlies::now)
    }

    pub fn fix(&self) -> Option<&Sample<Float>> {
        self.ready().and_then(CrowFlies::latest)
    }

    pub fn predictions(&self) -> &[predictor::Prediction<Float>] {
        self.ready().map_or(&[], CrowFlies::predictions)
    }

    pub fn speed_mps(&self) -> Option<Float> {
        self.ready().and_then(CrowFlies::speed_mps)
    }

    pub fn radius_metres(&self) -> Option<Float> {
        self.ready().map(CrowFlies::radius_metres)
    }

    pub fn crossings(&self) -> Option<&S::Crossings> {
        self.ready().map(CrowFlies::crossings)
    }

    fn ready(&self) -> Option<&CrowFlies<Float, S::Crossings>> {
        match &self.predictor {
            Predicting::Ready(predictor) => Some(predictor),
            Predicting::Waiting => None,
        }
    }

    pub fn charge(&self) -> Option<Charge> {
        self.battery.charge()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    Reset,
    Crossings(Vec<CrossingCompact<f64>>),
    Tick(DateTime<Utc>),
    Sentence(Sentence),
    Position(Sample<f64>),
    Battery(u16),
}

#[effect(typegen)]
pub enum Effect {
    Render(RenderOperation),
    Crossings(GetCrossings),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetCrossings;

impl Operation for GetCrossings {
    type Output = ();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Moved,
    Unchanged,
}

pub struct Lookout<S: Shell>(PhantomData<S>);

impl<S: Shell> Default for Lookout<S> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<S: Shell> Lookout<S> {
    fn absorb(&self, sentence: &Sentence, model: &mut Model<S>) -> Change {
        let Some(sample) = model.parser.absorb(sentence) else {
            return Change::Unchanged;
        };
        self.observe(Observed::Sampled(sample), model)
    }

    fn observe(&self, event: Observed<Float>, model: &mut Model<S>) -> Change {
        let Predicting::Ready(predictor) = &mut model.predictor else {
            return Change::Unchanged;
        };
        match predictor.observe(event) {
            Ok(()) => Change::Moved,
            Err(_) => Change::Unchanged,
        }
    }
}

impl<S: Shell> App for Lookout<S> {
    type Event = Event;
    type Model = Model<S>;
    type ViewModel = S::ViewModel;
    type Effect = Effect;
    type Capabilities = ();

    fn update(&self, event: Event, model: &mut Model<S>, _caps: &()) -> Command<Effect, Event> {
        let change = match event {
            Event::Reset => {
                *model = Model::default();
                return match model.predictor {
                    Predicting::Waiting => {
                        Command::all([render::render(), Command::notify_shell(GetCrossings).into()])
                    }
                    Predicting::Ready(_) => render::render(),
                };
            }
            Event::Crossings(points) => match (&model.predictor, S::received(points)) {
                (Predicting::Waiting, Some(crossings)) => {
                    model.predictor =
                        Predicting::Ready(CrowFlies::new(crossings, DEFAULT_RADIUS_METRES));
                    Change::Moved
                }
                _ => Change::Unchanged,
            },
            Event::Tick(now) => self.observe(Observed::Elapsed(now), model),
            Event::Sentence(sentence) => self.absorb(&sentence, model),
            Event::Position(reported) => match reported.to_precision() {
                Ok(sample) => self.observe(Observed::Sampled(sample), model),
                Err(_) => Change::Unchanged,
            },
            Event::Battery(millivolts) => {
                let before = model.battery.charge();
                model.battery.measured(millivolts);
                if model.battery.charge() == before {
                    Change::Unchanged
                } else {
                    Change::Moved
                }
            }
        };

        match change {
            Change::Moved => render::render(),
            Change::Unchanged => Command::done(),
        }
    }

    fn view(&self, model: &Model<S>) -> S::ViewModel {
        S::project(model)
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use crux_core::Core;
    use crux_core::bridge::BridgeWithSerializer;
    use domain::Gps;
    use geo_types::Point;
    use predictor::fixtures::{Fix, captured};

    use super::*;

    #[derive(Debug, Default, Clone, Copy)]
    struct Bare;

    #[derive(Debug, Default, Clone, Copy)]
    struct Late;

    #[derive(Debug, PartialEq)]
    struct State {
        now: Option<DateTime<Utc>>,
        latitude: Option<Float>,
        speed_mps: Option<Float>,
        charge: Option<Charge>,
        predicted: usize,
    }

    impl Shell for Bare {
        type ViewModel = State;
        type Crossings = Vec<CrossingCompact<Float>>;

        fn carried() -> Option<Self::Crossings> {
            Some(Vec::new())
        }

        fn received(points: Vec<CrossingCompact<f64>>) -> Option<Self::Crossings> {
            Some(taken(points))
        }

        fn project(model: &Model<Self>) -> State {
            State {
                now: model.now(),
                latitude: model.fix().map(Sample::latitude),
                speed_mps: model.speed_mps(),
                charge: model.charge(),
                predicted: model.predictions().len(),
            }
        }
    }

    impl Shell for Late {
        type ViewModel = State;
        type Crossings = Vec<CrossingCompact<Float>>;

        fn carried() -> Option<Self::Crossings> {
            None
        }

        fn received(points: Vec<CrossingCompact<f64>>) -> Option<Self::Crossings> {
            Some(taken(points))
        }

        fn project(model: &Model<Self>) -> State {
            State {
                now: model.now(),
                latitude: model.fix().map(Sample::latitude),
                speed_mps: model.speed_mps(),
                charge: model.charge(),
                predicted: model.predictions().len(),
            }
        }
    }

    fn taken(points: Vec<CrossingCompact<f64>>) -> Vec<CrossingCompact<Float>> {
        points
            .into_iter()
            .filter_map(|point| {
                CrossingCompact::at(point.id, point.latitude(), point.longitude()).ok()
            })
            .collect()
    }

    fn core() -> Core<Lookout<Bare>> {
        Core::new()
    }

    fn waiting() -> Core<Lookout<Late>> {
        Core::new()
    }

    fn instant() -> DateTime<Utc> {
        DateTime::from_timestamp(1_785_098_609, 0).expect("an instant")
    }

    const DRESDEN_LON: f64 = 13.7322;

    fn at_the_station() -> Gps<f64> {
        Gps::at(51.0403, 13.7322)
            .expect("on the globe")
            .with_accuracy_metres(Some(5.0))
            .with_speed_mps(Some(27.8))
    }

    fn crossing(id: u32, latitude: f64, longitude: f64) -> CrossingCompact<f64> {
        CrossingCompact::at(id, latitude, longitude).expect("on the globe")
    }

    fn reported(t: DateTime<Utc>, gps: Gps<f64>) -> Event {
        Event::Position(domain::Sample::new(t, gps))
    }

    #[test]
    fn a_shell_with_no_crossings_is_asked_for_them() {
        let core = waiting();

        let effects = core.process_event(Event::Reset);

        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Crossings(_)))
        );
    }

    #[test]
    fn a_shell_that_brought_its_own_is_asked_for_nothing() {
        let core = core();

        let effects = core.process_event(Event::Reset);

        assert!(matches!(effects.as_slice(), [Effect::Render(_)]));
    }

    #[test]
    fn starting_again_asks_for_the_crossings_again() {
        let core = waiting();
        core.process_event(Event::Reset);

        core.process_event(Event::Crossings(vec![crossing(1, 51.0503, 13.7322)]));

        let effects = core.process_event(Event::Reset);

        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Crossings(_)))
        );
        assert_eq!(core.view().predicted, 0);
    }

    #[test]
    fn a_fix_before_there_are_crossings_is_refused() {
        let core = waiting();

        let effects = core.process_event(reported(instant(), at_the_station()));

        assert_eq!(core.view().latitude, None);
        assert_eq!(core.view().now, None);
        assert!(effects.is_empty());
    }

    #[test]
    fn crossings_arriving_are_what_a_later_fix_is_measured_against() {
        let core = waiting();
        core.process_event(Event::Reset);

        core.process_event(Event::Crossings(vec![
            crossing(1, 51.0503, 13.7322),
            crossing(2, 60.0, 13.7322),
        ]));
        core.process_event(reported(instant(), at_the_station()));

        assert_eq!(core.view().predicted, 1);
    }

    #[test]
    fn crossings_arriving_before_the_shell_starts_are_taken() {
        let core = waiting();

        core.process_event(Event::Crossings(vec![crossing(1, 51.0503, 13.7322)]));
        core.process_event(reported(instant(), at_the_station()));

        assert_eq!(core.view().predicted, 1);
    }

    #[test]
    fn a_second_set_leaves_the_first_in_place() {
        let core = core();
        core.process_event(reported(instant(), at_the_station()));

        let effects = core.process_event(Event::Crossings(vec![crossing(1, 51.0503, 13.7322)]));

        assert_eq!(core.view().predicted, 0);
        assert!(effects.is_empty());
    }

    #[test]
    fn a_point_off_the_globe_is_dropped_rather_than_losing_the_set() {
        let core = waiting();

        let off_the_globe = CrossingCompact::new(1, geo_types::Point::new(13.7322, 91.0));

        core.process_event(Event::Crossings(vec![
            off_the_globe,
            crossing(2, 51.0503, 13.7322),
        ]));
        core.process_event(reported(instant(), at_the_station()));

        assert_eq!(core.view().predicted, 1);
    }

    #[test]
    fn nothing_is_known_before_an_event() {
        let core = core();

        assert_eq!(
            core.view(),
            State {
                now: None,
                latitude: None,
                speed_mps: None,
                charge: None,
                predicted: 0,
            }
        );
    }

    #[test]
    fn a_position_becomes_the_fix_everything_is_measured_from() {
        let core = core();

        let effects = core.process_event(reported(instant(), at_the_station()));

        let view = core.view();
        assert_eq!(view.now, Some(instant()));
        assert_eq!(view.speed_mps, Some(27.8));
        assert!((view.latitude.expect("a fix") - 51.0403).abs() < 1e-4);
        assert!(matches!(effects.as_slice(), [Effect::Render(_)]));
    }

    #[test]
    fn a_position_and_a_sentence_move_the_same_fix() {
        let core = core();
        let sentence = Fix::at(20, 43, 29, 51.0403, 13.7322);
        core.process_event(Event::Sentence(sentence.rmc()));

        core.process_event(reported(
            sentence.t() + TimeDelta::seconds(1),
            Gps {
                position: Point::new(DRESDEN_LON, 52.0),
                ..at_the_station()
            },
        ));

        assert!((core.view().latitude.expect("a fix") - 52.0).abs() < 1e-4);
    }

    #[test]
    fn a_position_off_the_globe_changes_nothing() {
        let core = core();
        core.process_event(reported(instant(), at_the_station()));

        let effects = core.process_event(reported(
            instant() + TimeDelta::seconds(1),
            Gps {
                position: Point::new(DRESDEN_LON, 91.0),
                ..at_the_station()
            },
        ));

        assert!((core.view().latitude.expect("a fix") - 51.0403).abs() < 1e-4);
        assert!(effects.is_empty());
    }

    #[test]
    fn a_position_before_the_last_one_is_taken_after_starting_again() {
        let core = core();
        core.process_event(reported(instant(), at_the_station()));

        core.process_event(Event::Reset);
        core.process_event(reported(
            instant() - TimeDelta::seconds(3_600),
            Gps {
                position: Point::new(DRESDEN_LON, 52.0),
                ..at_the_station()
            },
        ));

        assert!((core.view().latitude.expect("a fix") - 52.0).abs() < 1e-4);
    }

    #[test]
    fn a_position_behind_the_clock_is_refused() {
        let core = core();
        core.process_event(reported(instant(), at_the_station()));

        let effects = core.process_event(reported(
            instant() - TimeDelta::seconds(1),
            Gps {
                position: Point::new(DRESDEN_LON, 52.0),
                ..at_the_station()
            },
        ));

        assert!((core.view().latitude.expect("a fix") - 51.0403).abs() < 1e-4);
        assert!(effects.is_empty());
    }

    #[test]
    fn a_position_without_a_speed_leaves_one_derived_from_the_fix_before() {
        let core = core();
        core.process_event(reported(
            instant(),
            Gps {
                speed_mps: None,
                ..at_the_station()
            },
        ));

        core.process_event(reported(
            instant() + TimeDelta::seconds(10),
            Gps {
                position: Point::new(DRESDEN_LON, 51.0503),
                speed_mps: None,
                ..at_the_station()
            },
        ));

        let derived = core.view().speed_mps.expect("a derived speed");
        assert!((derived - 111.0).abs() < 20.0, "{derived}m/s");
    }

    #[test]
    fn a_sentence_that_is_not_one_changes_nothing() {
        let core = core();

        let effects = core.process_event(Event::Sentence(captured("$GPRMC,nonsense*00")));

        assert!(effects.is_empty());
    }

    #[test]
    fn a_reported_position_is_written_as_the_shell_sends_it() {
        let event = reported(instant(), at_the_station());

        let json = serde_json::to_string(&event).expect("serialize");

        assert_eq!(
            json,
            r#"{"Position":{"t":"2026-07-26T20:43:29Z","gps":{"lat":51.0403,"lon":13.7322,"alt":null,"acc":5.0,"speed":27.8,"heading":null}}}"#
        );
    }

    #[test]
    fn the_request_for_crossings_reaches_a_shell_as_json() {
        let bridge: BridgeWithSerializer<Lookout<Late>> = BridgeWithSerializer::new(Core::new());
        let mut requests = Vec::new();

        bridge
            .process_event(
                &mut serde_json::Deserializer::from_str(r#""Reset""#),
                &mut serde_json::Serializer::new(&mut requests),
            )
            .expect("an event this core knows");

        assert_eq!(
            String::from_utf8(requests).expect("utf-8"),
            r#"[{"id":0,"effect":{"Render":null}},{"id":1,"effect":{"Crossings":null}}]"#
        );
    }

    #[test]
    fn a_battery_reading_is_judged_here_rather_than_by_the_shell() {
        let core = core();

        core.process_event(Event::Battery(4_200));

        assert_eq!(core.view().charge, Some(Charge::Full));
    }
}
