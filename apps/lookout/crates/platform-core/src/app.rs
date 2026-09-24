//! The state every platform drives, and what each event does to it.
//!
//! Each kind of platform has its own app — [`crate::standalone`] and [`crate::connected`] —
//! because crux builds one effect enum per app. The two kinds answer different ones. What
//! they hold and what they are told are here, once: one [`Event`], one [`Model`], and one
//! transition per event. The state cannot fork: there is only one of it.

use chrono::{DateTime, Utc};
use crux_core::{
    Command, Request,
    render::{self, RenderOperation},
};
use domain::{CrossingCompact, Sample};
use predictor::{
    Crossings, CrowFlies, DEFAULT_RADIUS_METRES, Event as Observed, Parser, Predict, Sentence,
};
use serde::{Deserialize, Serialize};

use crate::Float;
use crate::battery::{Battery, Charge};

/// What every platform brings to the core: a set to scan, and a view to draw.
///
/// The device holds thousands of points in flash and draws a 13-character panel; the browser
/// holds a `Vec` it fetched and draws a canvas. Everything between the two — the parsing, the
/// scan, the clock — is the same code either way.
///
/// What a platform can *do* is the other half, and the two traits extending this one split
/// it. A [`Standalone`](crate::Standalone) platform carries its crossings; the core asks a
/// [`Connected`](crate::Connected) one for a set it lacks. Every platform is one or the
/// other, and which it is decides what its shell answers.
pub trait Shell: Sized + 'static {
    /// What [`crux_core::App::view`] answers. A panel of strings, a structure of positions,
    /// whatever the shell can draw.
    type ViewModel;

    /// How this platform holds the set it scans. The device reads packed columns where they
    /// lie; anything with room to spare holds a `Vec`.
    type Crossings: Crossings<Float>;

    /// What the shell shows, from the state the core holds.
    fn project(model: &Model<Self>) -> Self::ViewModel;
}

/// The state every shell drives, whatever it draws.
///
/// Nothing here is public: a projection reads it through the methods below, so a view cannot
/// reach past what the core says it knows.
pub struct Model<S: Shell> {
    parser: Parser<Float>,
    battery: Battery,
    predictor: Predicting<S>,
}

/// Whether there is anything to predict against yet.
///
/// A predictor is built from its crossings and keeps them for as long as it lives. A core with
/// no set has no predictor, rather than a predictor scanning nothing. Everything measured
/// against the set — the clock, the fix, the predictions — arrives with it.
enum Predicting<S: Shell> {
    /// No crossings, and so nothing observed: whatever a shell sends before it has answered
    /// has nowhere to be measured against, and is refused.
    Waiting,
    Ready(CrowFlies<Float, S::Crossings>),
}

/// Waiting to be told, which is where a connected platform starts. One carrying its own set
/// starts at [`Model::over`] instead.
impl<S: Shell> Default for Model<S> {
    fn default() -> Self {
        Self {
            parser: Parser::new(),
            battery: Battery::default(),
            predictor: Predicting::Waiting,
        }
    }
}

/// What a projection may ask the model. The clock and the fix come from the predictor rather
/// than sitting beside it. Every position a view reports is one the scan ran from.
impl<S: Shell> Model<S> {
    /// The time the core is working to: a receiver's, where one has reported, and otherwise
    /// the shell's own.
    pub fn now(&self) -> Option<DateTime<Utc>> {
        self.ready().and_then(CrowFlies::now)
    }

    /// The fix the current predictions were made from.
    pub fn fix(&self) -> Option<&Sample<Float>> {
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

/// What each event does to the state. One rule per event about whether it moved anything
/// worth drawing, the same on either kind of platform.
impl<S: Shell> Model<S> {
    /// The model as it starts on a platform carrying its own crossings.
    pub(crate) fn over(crossings: S::Crossings) -> Self {
        Self {
            predictor: Predicting::Ready(CrowFlies::new(crossings, DEFAULT_RADIUS_METRES)),
            ..Self::default()
        }
    }

    /// Takes a set to predict against, answering whether anything shown has moved.
    ///
    /// A predictor keeps the crossings it was built with, so a set arriving for one that
    /// already has them changes nothing.
    pub(crate) fn given(&mut self, crossings: S::Crossings) -> Change {
        match self.predictor {
            Predicting::Waiting => {
                self.predictor =
                    Predicting::Ready(CrowFlies::new(crossings, DEFAULT_RADIUS_METRES));
                Change::Moved
            }
            Predicting::Ready(_) => Change::Unchanged,
        }
    }

    /// Takes one sentence off the wire, answering whether anything shown has moved.
    ///
    /// A sentence completing a fix moves to it and predicts afresh from it. One completing
    /// nothing leaves the last fix and prediction in place: one the receiver emits before it
    /// has a fix, one whose checksum fails, one repeating what is known.
    ///
    /// That matters at a dozen sentences a second. Otherwise one position would scan the
    /// whole set, and redraw the screen, a dozen times.
    pub(crate) fn absorb(&mut self, sentence: &Sentence) -> Change {
        let Some(sample) = self.parser.absorb(sentence) else {
            return Change::Unchanged;
        };
        self.observe(Observed::Sampled(sample))
    }

    /// Takes the time as the shell reads it, so a countdown shortens between fixes.
    pub(crate) fn elapsed(&mut self, now: DateTime<Utc>) -> Change {
        self.observe(Observed::Elapsed(now))
    }

    /// Takes a fix from a shell with no receiver to read.
    ///
    /// A coordinate off the globe is refused as [`Model::absorb`] refuses a corrupt sentence:
    /// it leaves the last fix and its predictions where they were.
    pub(crate) fn positioned(&mut self, reported: Sample<f64>) -> Change {
        match reported.to_precision() {
            Ok(sample) => self.observe(Observed::Sampled(sample)),
            Err(_) => Change::Unchanged,
        }
    }

    /// Takes a terminal voltage in millivolts, answering whether what it means has changed.
    /// The core decides what a voltage means, not the shell — see [`crate::battery`].
    pub(crate) fn measured(&mut self, millivolts: u16) -> Change {
        let before = self.battery.charge();
        self.battery.measured(millivolts);
        if self.battery.charge() == before {
            Change::Unchanged
        } else {
            Change::Moved
        }
    }

    /// Applies one event to the predictor, answering whether anything shown has moved.
    ///
    /// An event dated before the clock is refused, and leaves everything as it was. So is one
    /// arriving before there are crossings to measure it against: there is nowhere to put it,
    /// and nothing for it to move.
    fn observe(&mut self, event: Observed<Float>) -> Change {
        let Predicting::Ready(predictor) = &mut self.predictor else {
            return Change::Unchanged;
        };
        match predictor.observe(event) {
            Ok(()) => Change::Moved,
            Err(_) => Change::Unchanged,
        }
    }
}

/// Everything a shell reports, and the set it answers a request with.
///
/// One enum for both kinds of platform, where the effects are two. A shell answers every
/// effect, so one it cannot perform costs it a dead arm. Nothing obliges a shell to send
/// every event, so one it never sends costs it nothing.
#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    /// Start again, knowing nothing. A shell sends this when it comes up, and again whenever
    /// what follows has nothing to do with what came before. A kiosk does so between the
    /// recordings it replays. The next journey happened before the last one, so the clock
    /// would otherwise refuse every fix in it.
    ///
    /// A platform carrying its crossings has them again; the core asks a connected one afresh.
    Reset,
    /// The crossings a shell was asked for. A shell may send these unasked. One that already
    /// has a set ignores them: a predictor keeps the crossings it was built with.
    Crossings(Vec<CrossingCompact<f64>>),
    /// The time, as the shell reads it, so a countdown shortens between fixes rather than
    /// waiting for the next one. A time behind what the receiver has already reported is
    /// refused. A shell with no real clock can send these or not, and the panel reads the
    /// same either way. The device is such a shell: no NTP, and no RTC.
    Tick(DateTime<Utc>),
    /// One sentence off the UART. The shell reads a line and checks it is one, so it refuses
    /// noise on the wire rather than passing it here.
    Sentence(Sentence),
    /// A fix from a shell with no receiver to read — a browser's geolocation, or a replay of
    /// one recorded. It arrives parsed, where a sentence arrives as text, and carries its own
    /// instant: whatever produced the fix dated it, not the shell.
    Position(Sample<f64>),
    /// The battery terminal voltage the shell measured, in millivolts. A shell with no
    /// battery to read, such as a browser, never sends one.
    Battery(u16),
}

/// Whether an event moved anything a shell shows, which is what decides a redraw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    Moved,
    Unchanged,
}

impl Change {
    /// What the core answers a shell with. An event that moved nothing raises no request at
    /// all. That is what stops a replay at speed from redrawing the canvas per sample.
    pub(crate) fn answered<E, Ev>(self) -> Command<E, Ev>
    where
        E: From<Request<RenderOperation>> + Send + 'static,
        Ev: Send + 'static,
    {
        match self {
            Change::Moved => render::render(),
            Change::Unchanged => Command::done(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use crux_core::Core;
    use domain::Gps;
    use geo_types::Point;
    use predictor::fixtures::captured;

    use super::*;
    use crate::fixtures::{Bare, DRESDEN_LON, at_the_station, instant, reported};
    use crate::standalone::Lookout;

    /// Every event but the two about crossings reaches the same state on either kind of
    /// platform, so these tests drive the simpler kind.
    fn core() -> Core<Lookout<Bare>> {
        Core::new()
    }

    #[test]
    fn nothing_is_known_before_an_event() {
        let core = core();
        let view = core.view();

        assert_eq!(view.now, None);
        assert_eq!(view.latitude, None);
        assert_eq!(view.speed_mps, None);
        assert_eq!(view.charge, None);
        assert_eq!(view.predicted, 0);
    }

    #[test]
    fn a_position_becomes_the_fix_everything_is_measured_from() {
        let core = core();

        let effects = core.process_event(reported(instant(), at_the_station()));

        let view = core.view();
        assert_eq!(view.now, Some(instant()));
        assert_eq!(view.speed_mps, Some(27.8));
        assert!((view.latitude.expect("a fix") - 51.0403).abs() < 1e-4);
        assert_eq!(effects.len(), 1);
    }

    /// A shell reading a receiver and a shell reading a browser feed the same state. So the
    /// second kind of event moves what the first kind left.
    #[test]
    fn a_position_and_a_sentence_move_the_same_fix() {
        let core = core();
        // The sentence carries its own instant, and the position that follows has to be
        // later, or the clock refuses it.
        let sentence = predictor::fixtures::Fix::at(20, 43, 29, 51.0403, 13.7322);
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

    /// As a corrupt sentence is refused: the last fix and its predictions stay as they were.
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

    /// A kiosk replaying one recording after another goes back in time between them, and what
    /// makes a fix stale is a newer fix. Starting again is how a shell says so.
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
    fn a_battery_reading_is_judged_here_rather_than_by_the_shell() {
        let core = core();

        core.process_event(Event::Battery(4_200));

        assert_eq!(core.view().charge, Some(Charge::Full));
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
}
