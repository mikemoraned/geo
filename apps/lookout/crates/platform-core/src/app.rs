//! The core itself: sentences, voltages and ticks in, whatever the shell draws out.

use core::marker::PhantomData;

use chrono::{DateTime, Utc};
use crux_core::{
    App, Command,
    macros::effect,
    render::{self, RenderOperation},
};
use predictor::{CrowFlies, DEFAULT_RADIUS_METRES, Event as Observed, Parser, Predict, Sentence};
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
