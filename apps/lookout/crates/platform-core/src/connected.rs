//! A platform that can call out, so the core asks it for a set it lacks.
//!
//! That is a browser, which fetches its crossings over the network, and later a board with a
//! radio. Its shell answers two effects: draw, and find a set.

use core::marker::PhantomData;

use crux_core::{
    App, Command,
    capability::Operation,
    macros::effect,
    render::{self, RenderOperation},
};
use domain::CrossingCompact;
use serde::{Deserialize, Serialize};

use crate::Float;
use crate::app::{Event, Model, Shell};

/// A platform that has no crossings until a shell sends them.
pub trait Connected: Shell {
    /// The set this platform makes of the points a shell sent.
    ///
    /// The core asks whenever a shell answers [`Event::Crossings`], and scans what comes back
    /// from then on.
    fn received(points: Vec<CrossingCompact<f64>>) -> Self::Crossings;
}

/// What the core asks a shell on such a platform to do: draw, and find a set.
///
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
/// Carries nothing: which set to send is the shell's business. The answer comes back as
/// [`Event::Crossings`] rather than as a response. A shell that has to fetch one can then let
/// the request go while it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetCrossings;

impl Operation for GetCrossings {
    type Output = ();
}

/// Every point the globe has room for, in the precision a scan measures in.
///
/// A point it has no room for is dropped rather than refusing the whole set, so one bad row
/// costs a page nothing but itself. A set arrives unchecked, since nothing on the wire refuses
/// one for holding an impossible coordinate.
pub fn on_the_globe(points: Vec<CrossingCompact<f64>>) -> Vec<CrossingCompact<Float>> {
    points
        .into_iter()
        .filter_map(|point| CrossingCompact::at(point.id, point.latitude(), point.longitude()).ok())
        .collect()
}

pub struct Lookout<S: Connected>(PhantomData<S>);

/// Written out rather than derived: a derive would ask the shell to be `Default` too. A
/// shell is a name for a platform, not a value anyone builds.
impl<S: Connected> Default for Lookout<S> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<S: Connected> App for Lookout<S> {
    type Event = Event;
    type Model = Model<S>;
    type ViewModel = S::ViewModel;
    type Effect = Effect;
    /// Unused: the returned `Command` describes an effect. This crux version requires the
    /// associated type, and later ones drop it.
    type Capabilities = ();

    /// The core asks for a render only where the shell would draw something different.
    fn update(&self, event: Event, model: &mut Model<S>, _caps: &()) -> Command<Effect, Event> {
        let change = match event {
            // The one event answered with something other than a render alone. Everything the
            // core knows came from what the shell told it, so starting again is the model as
            // it was built. Here that is nothing, and the core asks for the set afresh.
            Event::Reset => {
                *model = Model::default();
                return Command::all([
                    render::render(),
                    Command::notify_shell(GetCrossings).into(),
                ]);
            }
            Event::Crossings(points) => model.given(S::received(points)),
            Event::Tick(now) => model.elapsed(now),
            Event::Sentence(sentence) => model.absorb(&sentence),
            Event::Position(reported) => model.positioned(reported),
            Event::Battery(millivolts) => model.measured(millivolts),
        };
        change.answered()
    }

    fn view(&self, model: &Model<S>) -> S::ViewModel {
        S::project(model)
    }
}

#[cfg(test)]
mod tests {
    use crux_core::Core;
    use crux_core::bridge::BridgeWithSerializer;

    use super::*;
    use crate::fixtures::{Late, at_the_station, crossing, instant, reported};

    fn core() -> Core<Lookout<Late>> {
        Core::new()
    }

    #[test]
    fn a_platform_with_no_crossings_is_asked_for_them() {
        let core = core();

        let effects = core.process_event(Event::Reset);

        assert!(effects.iter().any(Effect::is_crossings));
    }

    #[test]
    fn starting_again_asks_for_the_crossings_again() {
        let core = core();
        core.process_event(Event::Reset);

        core.process_event(Event::Crossings(vec![crossing(1, 51.0503, 13.7322)]));

        // Starting again drops them, and asks for them afresh.
        let effects = core.process_event(Event::Reset);

        assert!(effects.iter().any(Effect::is_crossings));
        assert_eq!(core.view().predicted, 0);
    }

    /// A fix arriving before the set has nowhere to be measured, so the core refuses it, as
    /// it refuses a fix behind the clock. A shell answers before it starts reporting.
    #[test]
    fn a_fix_before_there_are_crossings_is_refused() {
        let core = core();

        let effects = core.process_event(reported(instant(), at_the_station()));

        assert_eq!(core.view().latitude, None);
        assert_eq!(core.view().now, None);
        assert!(effects.is_empty());
    }

    #[test]
    fn crossings_arriving_are_what_a_later_fix_is_measured_against() {
        let core = core();
        core.process_event(Event::Reset);

        core.process_event(Event::Crossings(vec![
            crossing(1, 51.0503, 13.7322),
            crossing(2, 60.0, 13.7322),
        ]));
        core.process_event(reported(instant(), at_the_station()));

        // The far one is outside the radius, so one of the two is predicted.
        assert_eq!(core.view().predicted, 1);
    }

    /// A shell may answer before the core asks.
    #[test]
    fn crossings_arriving_before_the_shell_starts_are_taken() {
        let core = core();

        core.process_event(Event::Crossings(vec![crossing(1, 51.0503, 13.7322)]));
        core.process_event(reported(instant(), at_the_station()));

        assert_eq!(core.view().predicted, 1);
    }

    /// A predictor keeps the crossings it was built with.
    #[test]
    fn a_second_set_leaves_the_first_in_place() {
        let core = core();
        core.process_event(Event::Crossings(vec![crossing(1, 51.0503, 13.7322)]));

        let effects = core.process_event(Event::Crossings(vec![
            crossing(2, 51.0403, 13.7322),
            crossing(3, 51.0413, 13.7322),
        ]));

        assert_eq!(core.view().crossings, 1);
        assert!(effects.is_empty());
    }

    #[test]
    fn a_point_off_the_globe_is_dropped_rather_than_losing_the_set() {
        let core = core();

        core.process_event(Event::Crossings(vec![
            // Built unchecked, as one read off a wire is: nothing refuses a set for holding
            // this, so the platform drops the row.
            CrossingCompact::new(1, geo_types::Point::new(13.7322, 91.0)),
            crossing(2, 51.0503, 13.7322),
        ]));
        core.process_event(reported(instant(), at_the_station()));

        assert_eq!(core.view().predicted, 1);
    }

    /// The request as a shell with no generated bindings sees it. An element matches on the
    /// effect's name, so the name is part of what the core promises.
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
}
