use core::marker::PhantomData;

use chrono::{DateTime, Utc};
use crux_core::{
    App, Command,
    capability::Operation,
    macros::effect,
    render::{self, RenderOperation},
};
use domain::{Bbox, CrossingCompact, Sample};
use geo_types::Point;
use predictor::{
    Crossings, CrowFlies, DEFAULT_RADIUS, Event as Observed, Parser, Predict, Sentence,
};
use serde::{Deserialize, Serialize};

use crate::Float;
use crate::battery::{Battery, Charge};
use crate::lines::{Nearby, RequestId, Wkb};
use crate::trace::Trace;

pub trait Shell: Sized + 'static {
    type ViewModel;

    type Crossings: Crossings<Float> + Default;

    fn carried() -> Option<Self::Crossings>;

    fn received(points: Vec<CrossingCompact<f64>>) -> Option<Self::Crossings>;

    fn draws_lines() -> bool;

    fn project(model: &Model<Self>) -> Self::ViewModel;
}

pub struct Model<S: Shell> {
    parser: Parser<Float>,
    battery: Battery,
    predictor: Predicting<S>,
    nearby: Nearby,
    trace: Trace,
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
                Some(crossings) => Predicting::Ready(CrowFlies::new(crossings, DEFAULT_RADIUS)),
                None => Predicting::Waiting,
            },
            nearby: Nearby::default(),
            trace: Trace::default(),
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

    pub fn lines(&self) -> &[Wkb] {
        self.nearby.shown()
    }

    pub fn trace(&self) -> impl Iterator<Item = Point<Float>> + '_ {
        self.trace.positions()
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
    Lines { id: RequestId, lines: Vec<Wkb> },
}

#[effect(typegen)]
pub enum Effect {
    Render(RenderOperation),
    Crossings(GetCrossings),
    Lines(GetLines),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetCrossings;

impl Operation for GetCrossings {
    type Output = ();
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GetLines {
    pub id: RequestId,
    pub area: Bbox,
}

impl Operation for GetLines {
    type Output = ();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
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
        let sampled = match event {
            Observed::Sampled(sample) => Some(sample),
            Observed::Elapsed(_) => None,
        };
        match predictor.observe(event) {
            Ok(()) => {
                if let Some(sample) = sampled
                    && S::draws_lines()
                {
                    model.trace.followed(sample);
                }
                Change::Moved
            }
            Err(_) => Change::Unchanged,
        }
    }

    fn lines_to_ask_for(&self, model: &mut Model<S>) -> Option<GetLines> {
        if !S::draws_lines() {
            return None;
        }
        let fix = model.fix()?.gps.position;
        let radius_metres = model.radius_metres()?;
        let centre = Point::new(f64::from(fix.x()), f64::from(fix.y()));
        model
            .nearby
            .follow(centre, f64::from(radius_metres))
            .ok()
            .flatten()
    }
}

impl<S: Shell> App for Lookout<S> {
    type Event = Event;
    type Model = Model<S>;
    type ViewModel = S::ViewModel;
    type Effect = Effect;

    fn update(&self, event: Event, model: &mut Model<S>) -> Command<Effect, Event> {
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
                    model.predictor = Predicting::Ready(CrowFlies::new(crossings, DEFAULT_RADIUS));
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
            Event::Lines { id, lines } => model.nearby.received(id, lines),
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
            Change::Moved => match self.lines_to_ask_for(model) {
                Some(request) => {
                    Command::all([render::render(), Command::notify_shell(request).into()])
                }
                None => render::render(),
            },
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
    use crux_core::bridge::{Bridge, JsonFfiFormat};
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
        traced: usize,
    }

    #[derive(Debug, PartialEq)]
    struct Drawn {
        lines: Vec<Wkb>,
        trace: Vec<Point<Float>>,
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

        fn draws_lines() -> bool {
            false
        }

        fn project(model: &Model<Self>) -> State {
            State {
                now: model.now(),
                latitude: model.fix().map(Sample::latitude),
                speed_mps: model.speed_mps(),
                charge: model.charge(),
                predicted: model.predictions().len(),
                traced: model.trace().count(),
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

        fn draws_lines() -> bool {
            false
        }

        fn project(model: &Model<Self>) -> State {
            State {
                now: model.now(),
                latitude: model.fix().map(Sample::latitude),
                speed_mps: model.speed_mps(),
                charge: model.charge(),
                predicted: model.predictions().len(),
                traced: model.trace().count(),
            }
        }
    }

    #[derive(Debug, Default, Clone, Copy)]
    struct Drawing;

    impl Shell for Drawing {
        type ViewModel = Drawn;
        type Crossings = Vec<CrossingCompact<Float>>;

        fn carried() -> Option<Self::Crossings> {
            Some(Vec::new())
        }

        fn received(points: Vec<CrossingCompact<f64>>) -> Option<Self::Crossings> {
            Some(taken(points))
        }

        fn draws_lines() -> bool {
            true
        }

        fn project(model: &Model<Self>) -> Drawn {
            Drawn {
                lines: model.lines().to_vec(),
                trace: model.trace().collect(),
            }
        }
    }

    fn drawing() -> Core<Lookout<Drawing>> {
        Core::new()
    }

    fn line() -> Wkb {
        Wkb::new(vec![
            1, 2, 0, 0, 0, 2, 0, 0, 0, 246, 40, 92, 143, 194, 117, 43, 64, 133, 235, 81, 184, 30,
            133, 73, 64, 123, 20, 174, 71, 225, 122, 43, 64, 102, 102, 102, 102, 102, 134, 73, 64,
        ])
        .expect("a line in WKB")
    }

    fn asked(effects: &[Effect]) -> Option<GetLines> {
        effects.iter().find_map(|effect| match effect {
            Effect::Lines(request) => Some(request.operation),
            _ => None,
        })
    }

    fn north_of_the_station(metres: f64) -> Gps<f64> {
        Gps {
            position: Point::new(DRESDEN_LON, 51.0403 + metres / 111_195.0),
            ..at_the_station()
        }
    }

    fn assert_near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-3,
            "{actual} is not {expected}"
        );
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
                traced: 0,
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
        let bridge: Bridge<Lookout<Late>, JsonFfiFormat> = Bridge::new(Core::new());
        let mut requests = Vec::new();

        bridge
            .update(br#""Reset""#, &mut requests)
            .expect("an event this core knows");

        assert_eq!(
            String::from_utf8(requests).expect("utf-8"),
            r#"[{"id":0,"effect":{"Render":null}},{"id":1,"effect":{"Crossings":null}}]"#
        );
    }

    #[test]
    fn lines_arrive_from_a_shell_as_wkb_in_base64() {
        let event: Event = serde_json::from_str(
            r#"{"Lines":{"id":"6ba7b810-9dad-51d1-80b4-00c04fd430c8","lines":["AQIAAAACAAAA9ihcj8J1K0CF61G4HoVJQHsUrkfheitAZmZmZmaGSUA="]}}"#,
        )
        .expect("an event this core knows");

        let Event::Lines { id, lines } = event else {
            panic!("lines, not {event:?}");
        };
        assert_eq!(
            serde_json::to_string(&id).expect("serialize"),
            r#""6ba7b810-9dad-51d1-80b4-00c04fd430c8""#
        );
        assert_eq!(lines, vec![line()]);
    }

    #[test]
    fn the_request_for_lines_carries_its_id_and_names_its_area_as_a_box() {
        let request = GetLines {
            id: serde_json::from_str(r#""6ba7b810-9dad-51d1-80b4-00c04fd430c8""#).expect("an id"),
            area: Bbox::new(13.66, 51.0, 13.81, 51.09).expect("a box"),
        };

        assert_eq!(
            serde_json::to_string(&request).expect("serialize"),
            r#"{"id":"6ba7b810-9dad-51d1-80b4-00c04fd430c8","area":{"xmin":13.66,"ymin":51.0,"xmax":13.81,"ymax":51.09}}"#
        );
    }

    #[test]
    fn a_fix_asks_for_the_lines_three_radii_around_it() {
        let core = drawing();

        let effects = core.process_event(reported(instant(), at_the_station()));

        let area = asked(&effects).expect("a request for lines").area;
        assert_near(area.min().x, 13.5177);
        assert_near(area.min().y, 50.9054);
        assert_near(area.max().x, 13.9467);
        assert_near(area.max().y, 51.1752);
    }

    #[test]
    fn a_fix_keeping_two_radii_inside_the_box_asks_for_nothing() {
        let core = drawing();
        core.process_event(reported(instant(), at_the_station()));

        let effects = core.process_event(reported(
            instant() + TimeDelta::seconds(10),
            north_of_the_station(4_000.0),
        ));

        assert!(matches!(effects.as_slice(), [Effect::Render(_)]));
    }

    #[test]
    fn a_fix_whose_two_radii_leave_the_box_asks_for_another_around_it() {
        let core = drawing();
        core.process_event(reported(instant(), at_the_station()));

        let effects = core.process_event(reported(
            instant() + TimeDelta::seconds(10),
            north_of_the_station(6_000.0),
        ));

        let area = asked(&effects).expect("a request for lines").area;
        assert_near(area.min().y, 50.9594);
        assert_near(area.max().y, 51.2292);
    }

    #[test]
    fn lines_answering_the_request_are_shown() {
        let core = drawing();
        let request = asked(&core.process_event(reported(instant(), at_the_station())))
            .expect("a request for lines");

        let effects = core.process_event(Event::Lines {
            id: request.id,
            lines: vec![line()],
        });

        assert_eq!(core.view().lines, vec![line()]);
        assert!(matches!(effects.as_slice(), [Effect::Render(_)]));
    }

    #[test]
    fn lines_answering_an_earlier_request_are_dropped() {
        let core = drawing();
        let earlier = asked(&core.process_event(reported(instant(), at_the_station())))
            .expect("a request for lines");
        core.process_event(reported(
            instant() + TimeDelta::seconds(10),
            north_of_the_station(6_000.0),
        ));

        let effects = core.process_event(Event::Lines {
            id: earlier.id,
            lines: vec![line()],
        });

        assert!(core.view().lines.is_empty());
        assert!(effects.is_empty());
    }

    #[test]
    fn starting_again_forgets_the_lines_and_asks_afresh() {
        let core = drawing();
        let request = asked(&core.process_event(reported(instant(), at_the_station())))
            .expect("a request for lines");
        core.process_event(Event::Lines {
            id: request.id,
            lines: vec![line()],
        });

        core.process_event(Event::Reset);
        let effects = core.process_event(reported(instant(), at_the_station()));

        assert!(core.view().lines.is_empty());
        assert_eq!(asked(&effects).map(|again| again.area), Some(request.area));
    }

    #[test]
    fn lines_answering_a_request_from_before_starting_again_are_dropped() {
        let core = drawing();
        let before = asked(&core.process_event(reported(instant(), at_the_station())))
            .expect("a request for lines");
        core.process_event(Event::Reset);
        core.process_event(reported(instant(), at_the_station()));

        let effects = core.process_event(Event::Lines {
            id: before.id,
            lines: vec![line()],
        });

        assert!(core.view().lines.is_empty());
        assert!(effects.is_empty());
    }

    #[test]
    fn a_shell_that_draws_no_lines_is_never_asked_for_them() {
        let core = core();

        let effects = core.process_event(reported(instant(), at_the_station()));

        assert_eq!(asked(&effects), None);
    }

    #[test]
    fn each_fix_a_drawing_shell_is_sent_joins_its_trace() {
        let core = drawing();

        core.process_event(reported(instant(), at_the_station()));
        core.process_event(reported(
            instant() + TimeDelta::seconds(10),
            north_of_the_station(1_000.0),
        ));

        let latitudes: Vec<Float> = core.view().trace.iter().map(|at| at.y()).collect();
        assert_eq!(latitudes.len(), 2);
        assert!((latitudes[0] - 51.0403).abs() < 1e-4);
        assert!((latitudes[1] - 51.0493).abs() < 1e-4);
    }

    #[test]
    fn a_refused_fix_stays_out_of_the_trace() {
        let core = drawing();
        core.process_event(reported(instant(), at_the_station()));

        core.process_event(reported(
            instant() - TimeDelta::seconds(1),
            north_of_the_station(1_000.0),
        ));

        assert_eq!(core.view().trace.len(), 1);
    }

    #[test]
    fn starting_again_forgets_the_trace() {
        let core = drawing();
        core.process_event(reported(instant(), at_the_station()));

        core.process_event(Event::Reset);

        assert!(core.view().trace.is_empty());
    }

    #[test]
    fn a_shell_that_draws_no_lines_keeps_no_trace() {
        let core = core();

        core.process_event(reported(instant(), at_the_station()));

        assert_eq!(core.view().traced, 0);
    }

    #[test]
    fn a_battery_reading_is_judged_here_rather_than_by_the_shell() {
        let core = core();

        core.process_event(Event::Battery(4_200));

        assert_eq!(core.view().charge, Some(Charge::Full));
    }
}
