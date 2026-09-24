//! A platform that carries its own crossings.
//!
//! It asks for nothing and needs only somewhere to send what it draws. Its shell answers one
//! effect, and its loop has one arm to write. That is the board: the whole set in flash, and
//! a panel.

use core::marker::PhantomData;

use crux_core::{App, Command, macros::effect, render::RenderOperation};

use crate::app::{Change, Event, Model, Shell};

/// A platform whose crossings are in place before anything runs.
pub trait Standalone: Shell {
    /// The set this platform holds.
    ///
    /// The core asks when it starts, and again whenever it starts over. A platform with no
    /// room to copy its points answers with a view of where they lie.
    fn carried() -> Self::Crossings;
}

/// What the core asks a shell on such a platform to do: draw.
///
/// Nothing to fetch means nothing else, so a loop over these has no arm for an effect that
/// never arrives.
#[effect]
pub enum Effect {
    Render(RenderOperation),
}

/// The model of a platform that carries its own set: the same [`Model`], born ready.
///
/// A type of its own because `Default` is one function per type, and the two kinds of platform
/// start differently. This one starts with the set it already has, the other with nothing
/// until it has asked. A shell never names it: what [`Shell::project`] reads is [`Model`].
pub struct Carried<S: Standalone>(Model<S>);

impl<S: Standalone> Default for Carried<S> {
    fn default() -> Self {
        Self(Model::over(S::carried()))
    }
}

pub struct Lookout<S: Standalone>(PhantomData<S>);

/// Written out rather than derived: a derive would ask the shell to be `Default` too. A
/// shell is a name for a platform, not a value anyone builds.
impl<S: Standalone> Default for Lookout<S> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<S: Standalone> App for Lookout<S> {
    type Event = Event;
    type Model = Carried<S>;
    type ViewModel = S::ViewModel;
    type Effect = Effect;
    /// Unused: the returned `Command` describes an effect. This crux version requires the
    /// associated type, and later ones drop it.
    type Capabilities = ();

    /// The core asks for a render only where the shell would draw something different.
    fn update(&self, event: Event, model: &mut Carried<S>, _caps: &()) -> Command<Effect, Event> {
        let change = match event {
            // Everything the core knows came from what the shell told it, so starting again
            // is the model as it was built. Here that is the set the platform carries, in
            // place again as it was at the start.
            Event::Reset => {
                *model = Carried::default();
                Change::Moved
            }
            // A predictor keeps the crossings it was built with, and this platform's were in
            // place before anything ran. Nothing sent to it can replace them.
            Event::Crossings(_) => Change::Unchanged,
            Event::Tick(now) => model.0.elapsed(now),
            Event::Sentence(sentence) => model.0.absorb(&sentence),
            Event::Position(reported) => model.0.positioned(reported),
            Event::Battery(millivolts) => model.0.measured(millivolts),
        };
        change.answered()
    }

    fn view(&self, model: &Carried<S>) -> S::ViewModel {
        S::project(&model.0)
    }
}

#[cfg(test)]
mod tests {
    use crux_core::Core;

    use super::*;
    use crate::fixtures::{Bare, at_the_station, crossing, instant, reported};

    fn core() -> Core<Lookout<Bare>> {
        Core::new()
    }

    /// The set is there from the moment the core is built, so coming up costs a draw and
    /// nothing else.
    #[test]
    fn a_platform_that_brought_its_own_is_asked_for_nothing() {
        let core = core();

        let effects = core.process_event(Event::Reset);

        assert!(matches!(effects.as_slice(), [Effect::Render(_)]));
    }

    /// The shared rule, where it is total: this model is always ready, so it always keeps the
    /// crossings it was built with.
    #[test]
    fn a_set_sent_to_a_platform_carrying_one_changes_nothing() {
        let core = core();
        core.process_event(reported(instant(), at_the_station()));

        let effects = core.process_event(Event::Crossings(vec![crossing(1, 51.0503, 13.7322)]));

        assert_eq!(core.view().predicted, 0);
        assert!(effects.is_empty());
    }
}
