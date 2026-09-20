//! The core itself: sentences, voltages and ticks in, whatever the shell draws out.

use core::marker::PhantomData;

use chrono::{DateTime, Utc};
use crux_core::{
    App, Command,
    macros::effect,
    render::{self, RenderOperation},
};
use model::Gps;
use predictor::{
    CrowFlies, DEFAULT_RADIUS_METRES, Event as Observed, Parser, Predict, Sample, Sentence,
};
use serde::{Deserialize, Serialize};

use crate::Float;
use crate::battery::{Battery, Charge};
use crate::pointset::PointSet;

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

    /// The crossings to predict against, read once when the model is built.
    fn crossings() -> PointSet<'static>;

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
    predictor: CrowFlies<Float, PointSet<'static>>,
    shell: PhantomData<S>,
}

/// The crossings are borrowed once, not per scan: they are the same bytes for the life of the
/// binary.
impl<S: Shell> Default for Model<S> {
    fn default() -> Self {
        Self {
            parser: Parser::new(),
            battery: Battery::default(),
            predictor: CrowFlies::new(S::crossings(), DEFAULT_RADIUS_METRES),
            shell: PhantomData,
        }
    }
}

/// What a projection may ask the model. The clock and the fix come from the predictor rather
/// than being held beside it, so a view cannot report a position the scan never ran from.
impl<S: Shell> Model<S> {
    /// The time the core is working to: a receiver's, where one has reported, and otherwise
    /// the shell's own.
    pub fn now(&self) -> Option<DateTime<Utc>> {
        self.predictor.now()
    }

    /// The fix the current predictions were made from.
    pub fn fix(&self) -> Option<&predictor::Sample<Float>> {
        self.predictor.latest()
    }

    /// The crossings inside the radius, nearest first.
    pub fn predictions(&self) -> &[predictor::Prediction<Float>] {
        self.predictor.predictions()
    }

    pub fn speed_mps(&self) -> Option<Float> {
        self.predictor.speed_mps()
    }

    /// The crossings predicted against
    pub fn crossings(&self) -> &PointSet<'static> {
        self.predictor.crossings()
    }

    /// How full the battery is, where a plausible voltage has been measured.
    pub fn charge(&self) -> Option<Charge> {
        self.battery.charge()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
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
    /// An event dated before the clock is refused, and leaves everything as it was.
    fn observe(&self, event: Observed<Float>, model: &mut Model<S>) -> Change {
        match model.predictor.observe(event) {
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
    use predictor::fixtures::{Fix, captured};

    use super::*;

    /// A shell that shows the state as it stands, so a test reads what the core holds rather
    /// than how some platform formats it. The projections themselves are tested where they
    /// live, against the screens they are for.
    #[derive(Debug, Default, Clone, Copy)]
    struct Bare;

    /// What the core knows, unformatted.
    #[derive(Debug, PartialEq)]
    struct State {
        now: Option<DateTime<Utc>>,
        latitude: Option<Float>,
        speed_mps: Option<Float>,
        charge: Option<Charge>,
    }

    /// No crossings, so nothing here predicts. What is predicted from a set is `predictor`'s
    /// to test, and it does; what an event does to the state is this crate's.
    impl Shell for Bare {
        type ViewModel = State;

        fn crossings() -> PointSet<'static> {
            PointSet::empty()
        }

        fn project(model: &Model<Self>) -> State {
            State {
                now: model.now(),
                latitude: model.fix().map(predictor::Sample::latitude),
                speed_mps: model.speed_mps(),
                charge: model.charge(),
            }
        }
    }

    fn core() -> Core<Lookout<Bare>> {
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
    fn nothing_is_known_before_an_event() {
        let core = core();

        assert_eq!(
            core.view(),
            State {
                now: None,
                latitude: None,
                speed_mps: None,
                charge: None,
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

    #[test]
    fn a_battery_reading_is_judged_here_rather_than_by_the_shell() {
        let core = core();

        core.process_event(Event::Battery(4_200));

        assert_eq!(core.view().charge, Some(Charge::Full));
    }
}
