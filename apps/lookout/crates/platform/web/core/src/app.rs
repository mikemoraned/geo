//! The counter itself: ticks in, a count out as text.

use chrono::{DateTime, Utc};
use crux_core::{
    App, Command,
    macros::effect,
    render::{self, RenderOperation},
};
use serde::{Deserialize, Serialize};

/// The count, and the time it was last moved by.
#[derive(Debug, Default)]
pub struct Model {
    count: u64,
    clock: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    /// The time, as the shell reads it. A time at or behind the one already counted is
    /// refused, so a repeated or reordered tick counts once, and the shell is free to send
    /// them as carelessly as a `setInterval` does.
    Tick(DateTime<Utc>),
}

/// `typegen` is what makes the effect serializable, which the bridge needs; it generates
/// nothing by itself. The generator it also declares stays behind this crate's `typegen`
/// feature, and nothing enables it — the shell reads the JSON directly.
#[effect(typegen)]
pub enum Effect {
    Render(RenderOperation),
}

/// What the shell draws: the count, already text, as the predictor's view will be already
/// projected.
#[derive(Debug, Serialize, Deserialize)]
pub struct ViewModel {
    pub count: String,
}

#[derive(Debug, Default)]
pub struct Counter;

impl App for Counter {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;
    /// Unused: an effect is described by the returned `Command`. The associated type is
    /// required by this crux version and dropped in later ones.
    type Capabilities = ();

    /// A render is asked for only where the count moved, so a shell ticking faster than the
    /// clock advances redraws nothing.
    fn update(&self, event: Event, model: &mut Model, _caps: &()) -> Command<Effect, Event> {
        let Event::Tick(now) = event;
        if model.clock.is_some_and(|last| now <= last) {
            return Command::done();
        }
        model.clock = Some(now);
        model.count += 1;
        render::render()
    }

    fn view(&self, model: &Model) -> ViewModel {
        ViewModel {
            count: model.count.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use crux_core::Core;

    use super::*;

    fn at(second: u32) -> DateTime<Utc> {
        DateTime::from_timestamp(i64::from(second), 0).expect("a time within range")
    }

    #[test]
    fn counts_nothing_before_the_first_tick() {
        let core: Core<Counter> = Core::new();

        assert_eq!(core.view().count, "0");
    }

    #[test]
    fn a_tick_moves_the_count_on() {
        let core: Core<Counter> = Core::new();

        core.process_event(Event::Tick(at(1)));
        core.process_event(Event::Tick(at(2)));

        assert_eq!(core.view().count, "2");
    }

    #[test]
    fn a_tick_that_counts_asks_for_a_render() {
        let core: Core<Counter> = Core::new();

        let effects = core.process_event(Event::Tick(at(1)));

        assert!(matches!(effects.as_slice(), [Effect::Render(_)]));
    }

    #[test]
    fn a_stale_tick_counts_for_nothing() {
        let core: Core<Counter> = Core::new();
        core.process_event(Event::Tick(at(2)));

        let effects = core.process_event(Event::Tick(at(2) - TimeDelta::seconds(1)));

        assert_eq!(core.view().count, "1");
        assert!(effects.is_empty());
    }

    #[test]
    fn a_repeated_tick_counts_once() {
        let core: Core<Counter> = Core::new();
        core.process_event(Event::Tick(at(2)));

        let effects = core.process_event(Event::Tick(at(2)));

        assert_eq!(core.view().count, "1");
        assert!(effects.is_empty());
    }
}
