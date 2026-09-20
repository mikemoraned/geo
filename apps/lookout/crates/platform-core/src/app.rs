//! The core itself: sentences, voltages and ticks in, whatever the shell draws out.

use core::marker::PhantomData;

use chrono::{DateTime, Utc};
use crux_core::{
    App, Command,
    capability::Operation,
    macros::effect,
    render::{self, RenderOperation},
};
use model::Gps;
use predictor::{
    Crossings, CrowFlies, DEFAULT_RADIUS_METRES, Event as Observed, Parser, Predict, Sample,
    Sentence,
};
use serde::{Deserialize, Serialize};

use crate::Float;
use crate::battery::{Battery, Charge};

/// What a shell brings to the core: where its crossings come from, and what it shows.
///
/// The two belong together because both are answers to "which shell is this": the device
/// carries its set in flash and draws a 13-character panel, and the browser fetches a set and
/// draws a canvas. Everything between the two — the parsing, the scan, the clock — is the
/// same code either way.
pub trait Shell: Sized + 'static {
    /// What [`App::view`] answers. A panel of strings, a structure of positions, whatever the
    /// shell can draw.
    type ViewModel;

    /// How this platform holds the set it scans. The device reads packed columns where they
    /// lie; anything with room to spare holds a `Vec`.
    type Crossings: Crossings<Float> + Default;

    /// The set this platform already has when the model is built. The device's is in flash and
    /// ready at boot; a browser has none until it has fetched one, and answers `None` so the
    /// core knows to ask.
    fn carried() -> Option<Self::Crossings>;

    /// A set the shell has answered [`Effect::Crossings`] with.
    ///
    /// `None` where this platform cannot be told its crossings, which is the device: its set
    /// is in flash, moving thousands of points into RAM is what keeping it there avoids, and
    /// nothing sends it any. A device that could be given a set over a connection would answer
    /// here instead.
    fn received(points: Vec<model::Crossing>) -> Option<Self::Crossings>;

    /// What the shell shows, from the state the core holds.
    fn project(model: &Model<Self>) -> Self::ViewModel;
}

/// The state every shell drives, whatever it draws.
///
/// Nothing here is public: a projection reads it through the methods below, so a view cannot
/// reach past what the core is willing to say it knows.
pub struct Model<S: Shell> {
    parser: Parser<Float>,
    battery: Battery,
    predictor: Predicting<S>,
}

/// Whether there is anything to predict against yet.
///
/// A predictor is built from its crossings and keeps them for as long as it lives, so a core
/// with no set has no predictor rather than a predictor scanning nothing. Everything measured
/// against the set — the clock, the fix, the predictions — arrives with it.
enum Predicting<S: Shell> {
    /// No crossings, and so nothing observed: whatever a shell sends before it has answered
    /// has nowhere to be measured against, and is refused.
    Waiting,
    Ready(CrowFlies<Float, S::Crossings>),
}

/// A shell with no set of its own starts scanning an empty one, which predicts nothing, until
/// it answers the request the core makes on [`Event::Start`].
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

/// What a projection may ask the model. The clock and the fix come from the predictor rather
/// than being held beside it, so a view cannot report a position the scan never ran from.
impl<S: Shell> Model<S> {
    /// The time the core is working to: a receiver's, where one has reported, and otherwise
    /// the shell's own.
    pub fn now(&self) -> Option<DateTime<Utc>> {
        self.ready().and_then(CrowFlies::now)
    }

    /// The fix the current predictions were made from.
    pub fn fix(&self) -> Option<&predictor::Sample<Float>> {
        self.ready().and_then(CrowFlies::latest)
    }

    /// The crossings inside the radius, nearest first.
    pub fn predictions(&self) -> &[predictor::Prediction<Float>] {
        self.ready().map_or(&[], CrowFlies::predictions)
    }

    pub fn speed_mps(&self) -> Option<Float> {
        self.ready().and_then(CrowFlies::speed_mps)
    }

    /// How far out the predictions reach, absent until there is a predictor to ask.
    pub fn radius_metres(&self) -> Option<Float> {
        self.ready().map(CrowFlies::radius_metres)
    }

    /// The crossings predicted against, absent until a shell has provided a set.
    pub fn crossings(&self) -> Option<&S::Crossings> {
        self.ready().map(CrowFlies::crossings)
    }

    fn ready(&self) -> Option<&CrowFlies<Float, S::Crossings>> {
        match &self.predictor {
            Predicting::Ready(predictor) => Some(predictor),
            Predicting::Waiting => None,
        }
    }

    /// How full the battery is, where a plausible voltage has been measured.
    pub fn charge(&self) -> Option<Charge> {
        self.battery.charge()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    /// The shell is up and driving the core. It answers with a request for crossings where it
    /// has none, so a shell that brought its own is asked for nothing.
    Start,
    /// The crossings a shell was asked for. A shell may send these unasked, and one that
    /// already has a set ignores them: a predictor keeps the crossings it was built with.
    Crossings(Vec<model::Crossing>),
    /// The time, as the shell reads it, so a countdown shortens between fixes rather than
    /// waiting for the next one. A time behind what the receiver has already reported is
    /// refused, so a shell with no real clock — no NTP and no RTC, which is the device — can
    /// send these or not, and the panel reads the same either way.
    Tick(DateTime<Utc>),
    /// One sentence off the UART. The shell reads a line and checks it is one, so noise on
    /// the wire is refused there rather than reaching here.
    Sentence(Sentence),
    /// A fix from a shell with no receiver to read — a browser's geolocation, or a replay of
    /// one recorded. It arrives parsed, where a sentence arrives as text, and carries its own
    /// instant because the fix is dated by whatever produced it rather than by the shell.
    Position { t: DateTime<Utc>, gps: Gps },
    /// The battery terminal voltage the shell measured, in millivolts. What it means is
    /// decided here, not there — see [`crate::battery`]. A shell with no battery to read,
    /// such as a browser, never sends one.
    Battery(u16),
}

/// `typegen` is what makes the effect serializable, which the web bridge needs; it generates
/// nothing by itself. The generator it also declares stays behind this crate's `typegen`
/// feature, and nothing enables it.
#[effect(typegen)]
pub enum Effect {
    Render(RenderOperation),
    Crossings(GetCrossings),
}

/// Asks the shell for the crossings to predict against.
///
/// Carries nothing: which set to send is the shell's business, and a shell reading flash has
/// nothing to look up. The answer comes back as [`Event::Crossings`] rather than as a
/// response, so a shell that has to fetch one need not hold the request open while it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetCrossings;

impl Operation for GetCrossings {
    type Output = ();
}

/// A reported fix as the predictor takes one.
///
/// # Errors
///
/// Returns an error where the coordinates are not on the globe.
fn fix(t: DateTime<Utc>, gps: &Gps) -> Result<Sample<Float>, predictor::CoordinateError> {
    Ok(Sample::at(t, gps.latitude, gps.longitude)?.with_speed_mps(gps.speed_mps))
}

/// Whether an event moved anything a shell shows, which is what decides a redraw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Moved,
    Unchanged,
}

pub struct Lookout<S: Shell>(PhantomData<S>);

/// Written out rather than derived: a derive would ask the shell to be `Default` too, and a
/// shell is a name for a projection, not a value anyone builds.
impl<S: Shell> Default for Lookout<S> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<S: Shell> Lookout<S> {
    /// Takes one sentence off the wire, answering whether anything shown has moved.
    ///
    /// A sentence completing a fix moves to it and predicts afresh from it. One completing
    /// nothing leaves the last fix and prediction in place: one the receiver emits before it
    /// has a fix, one whose checksum fails, one repeating what is known.
    ///
    /// That matters at a dozen sentences a second. Otherwise one position would scan the
    /// whole set, and redraw the screen, a dozen times.
    fn absorb(&self, sentence: &Sentence, model: &mut Model<S>) -> Change {
        let Some(sample) = model.parser.absorb(sentence) else {
            return Change::Unchanged;
        };
        self.observe(Observed::Sampled(sample), model)
    }

    /// Applies one event, answering whether anything shown has moved.
    ///
    /// An event dated before the clock is refused, and leaves everything as it was. So is one
    /// arriving before there are crossings to measure it against: there is nowhere to put it,
    /// and nothing it could move.
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
    /// Unused: an effect is described by the returned `Command`. The associated type is
    /// required by this crux version and dropped in later ones.
    type Capabilities = ();

    /// A render is asked for only where a shell would draw something different.
    fn update(&self, event: Event, model: &mut Model<S>, _caps: &()) -> Command<Effect, Event> {
        let change = match event {
            // The one event answered with something other than a render. A shell that brought
            // its own set is asked for nothing, and one already asked is not asked twice.
            Event::Start => {
                return match model.predictor {
                    Predicting::Waiting => Command::notify_shell(GetCrossings).into(),
                    Predicting::Ready(_) => Command::done(),
                };
            }
            // A predictor keeps the crossings it was built with, so a set arriving for one
            // that already has them changes nothing. Nor does one a shell cannot hold, which
            // is how the device answers: its set is in flash.
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
            // A coordinate off the globe is refused here as a corrupt sentence is refused in
            // `absorb`: it leaves the last fix and its predictions where they were.
            Event::Position { t, gps } => match fix(t, &gps) {
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
    use predictor::fixtures::{Fix, captured};

    use super::*;

    /// A shell that shows the state as it stands, so a test reads what the core holds rather
    /// than how some platform formats it. The projections themselves are tested where they
    /// live, against the screens they are for.
    ///
    /// It carries an empty set, as a device carries a full one: the predictor exists from the
    /// start and predicts nothing, which is what most of these tests want. [`Late`] is the one
    /// that has to be told.
    #[derive(Debug, Default, Clone, Copy)]
    struct Bare;

    /// A shell with no set of its own, as a browser is before it has fetched one.
    #[derive(Debug, Default, Clone, Copy)]
    struct Late;

    /// What the core knows, unformatted.
    #[derive(Debug, PartialEq)]
    struct State {
        now: Option<DateTime<Utc>>,
        latitude: Option<Float>,
        speed_mps: Option<Float>,
        charge: Option<Charge>,
        predicted: usize,
    }

    /// What is predicted from a set is `predictor`'s to test, and it does; what an event does
    /// to the state is this crate's.
    impl Shell for Bare {
        type ViewModel = State;
        type Crossings = Vec<predictor::Crossing<Float>>;

        fn carried() -> Option<Self::Crossings> {
            Some(Vec::new())
        }

        fn received(points: Vec<model::Crossing>) -> Option<Self::Crossings> {
            Some(taken(points))
        }

        fn project(model: &Model<Self>) -> State {
            State {
                now: model.now(),
                latitude: model.fix().map(predictor::Sample::latitude),
                speed_mps: model.speed_mps(),
                charge: model.charge(),
                predicted: model.predictions().len(),
            }
        }
    }

    impl Shell for Late {
        type ViewModel = State;
        type Crossings = Vec<predictor::Crossing<Float>>;

        fn carried() -> Option<Self::Crossings> {
            None
        }

        fn received(points: Vec<model::Crossing>) -> Option<Self::Crossings> {
            Some(taken(points))
        }

        fn project(model: &Model<Self>) -> State {
            State {
                now: model.now(),
                latitude: model.fix().map(predictor::Sample::latitude),
                speed_mps: model.speed_mps(),
                charge: model.charge(),
                predicted: model.predictions().len(),
            }
        }
    }

    /// A point the globe has no room for is dropped rather than refusing the whole set.
    fn taken(points: Vec<model::Crossing>) -> Vec<predictor::Crossing<Float>> {
        points
            .into_iter()
            .filter_map(|point| {
                predictor::Crossing::at(point.id, point.latitude(), point.longitude()).ok()
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

    /// Dresden Hauptbahnhof, at a train's speed.
    fn at_the_station() -> Gps {
        Gps {
            latitude: 51.0403,
            longitude: 13.7322,
            altitude_metres: None,
            accuracy_metres: 5.0,
            speed_mps: Some(27.8),
            heading_degrees: None,
        }
    }

    fn reported(t: DateTime<Utc>, gps: Gps) -> Event {
        Event::Position { t, gps }
    }

    #[test]
    fn a_shell_with_no_crossings_is_asked_for_them() {
        let core = waiting();

        let effects = core.process_event(Event::Start);

        assert!(matches!(effects.as_slice(), [Effect::Crossings(_)]));
    }

    #[test]
    fn a_shell_that_brought_its_own_is_asked_for_nothing() {
        let core = core();

        assert!(core.process_event(Event::Start).is_empty());
    }

    #[test]
    fn a_shell_that_has_answered_is_not_asked_again() {
        let core = waiting();
        core.process_event(Event::Start);

        core.process_event(Event::Crossings(vec![model::Crossing::new(
            1, 51.0503, 13.7322,
        )]));

        assert!(core.process_event(Event::Start).is_empty());
    }

    /// Nothing is measured against a set that is not there, so a fix sent before one is
    /// refused — as a fix behind the clock is. A shell answers before it starts reporting.
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
        core.process_event(Event::Start);

        core.process_event(Event::Crossings(vec![
            model::Crossing::new(1, 51.0503, 13.7322),
            model::Crossing::new(2, 60.0, 13.7322),
        ]));
        core.process_event(reported(instant(), at_the_station()));

        // The far one is outside the radius, so one of the two is predicted.
        assert_eq!(core.view().predicted, 1);
    }

    /// Answering unasked is not an error.
    #[test]
    fn crossings_arriving_before_the_shell_starts_are_taken() {
        let core = waiting();

        core.process_event(Event::Crossings(vec![model::Crossing::new(
            1, 51.0503, 13.7322,
        )]));
        core.process_event(reported(instant(), at_the_station()));

        assert_eq!(core.view().predicted, 1);
    }

    /// A predictor keeps the crossings it was built with.
    #[test]
    fn a_second_set_leaves_the_first_in_place() {
        let core = core();
        core.process_event(reported(instant(), at_the_station()));

        let effects = core.process_event(Event::Crossings(vec![model::Crossing::new(
            1, 51.0503, 13.7322,
        )]));

        assert_eq!(core.view().predicted, 0);
        assert!(effects.is_empty());
    }

    #[test]
    fn a_point_off_the_globe_is_dropped_rather_than_losing_the_set() {
        let core = waiting();

        core.process_event(Event::Crossings(vec![
            model::Crossing::new(1, 91.0, 13.7322),
            model::Crossing::new(2, 51.0503, 13.7322),
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

    /// A shell reading a receiver and a shell reading a browser feed the same state, so the
    /// second kind of event moves what the first kind left.
    #[test]
    fn a_position_and_a_sentence_move_the_same_fix() {
        let core = core();
        // The sentence carries its own instant, and the position that follows has to be
        // later, or the clock refuses it.
        let sentence = Fix::at(20, 43, 29, 51.0403, 13.7322);
        core.process_event(Event::Sentence(sentence.rmc()));

        core.process_event(reported(
            sentence.t() + TimeDelta::seconds(1),
            Gps {
                latitude: 52.0,
                ..at_the_station()
            },
        ));

        assert!((core.view().latitude.expect("a fix") - 52.0).abs() < 1e-4);
    }

    /// As a corrupt sentence is refused: the last fix and its predictions stay as they were.
    #[test]
    fn a_position_off_the_globe_changes_nothing() {
        let core = core();
        core.process_event(reported(instant(), at_the_station()));

        let effects = core.process_event(reported(
            instant() + TimeDelta::seconds(1),
            Gps {
                latitude: 91.0,
                ..at_the_station()
            },
        ));

        assert!((core.view().latitude.expect("a fix") - 51.0403).abs() < 1e-4);
        assert!(effects.is_empty());
    }

    #[test]
    fn a_position_behind_the_clock_is_refused() {
        let core = core();
        core.process_event(reported(instant(), at_the_station()));

        let effects = core.process_event(reported(
            instant() - TimeDelta::seconds(1),
            Gps {
                latitude: 52.0,
                ..at_the_station()
            },
        ));

        assert!((core.view().latitude.expect("a fix") - 51.0403).abs() < 1e-4);
        assert!(effects.is_empty());
    }

    /// A source reporting no speed still moves, and two fixes say how fast. The core reports
    /// the speed it predicted at, which is the derived one.
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
                latitude: 51.0503,
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

    /// A shell with no generated bindings writes this by hand, so the shape is part of what
    /// the core promises. The fix keeps the names every recorded one is stored under.
    #[test]
    fn a_reported_position_is_written_as_the_shell_sends_it() {
        let event = reported(instant(), at_the_station());

        let json = serde_json::to_string(&event).expect("serialize");

        assert_eq!(
            json,
            r#"{"Position":{"t":"2026-07-26T20:43:29Z","gps":{"lat":51.0403,"lon":13.7322,"alt":null,"acc":5.0,"speed":27.8,"heading":null}}}"#
        );
    }

    /// The request as a shell with no generated bindings sees it. An element matches on the
    /// effect's name, so the name is part of what the core promises.
    #[test]
    fn the_request_for_crossings_reaches_a_shell_as_json() {
        let bridge: BridgeWithSerializer<Lookout<Late>> = BridgeWithSerializer::new(Core::new());
        let mut requests = Vec::new();

        bridge
            .process_event(
                &mut serde_json::Deserializer::from_str(r#""Start""#),
                &mut serde_json::Serializer::new(&mut requests),
            )
            .expect("an event this core knows");

        assert_eq!(
            String::from_utf8(requests).expect("utf-8"),
            r#"[{"id":0,"effect":{"Crossings":null}}]"#
        );
    }

    #[test]
    fn a_battery_reading_is_judged_here_rather_than_by_the_shell() {
        let core = core();

        core.process_event(Event::Battery(4_200));

        assert_eq!(core.view().charge, Some(Charge::Full));
    }
}
