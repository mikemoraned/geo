//! The behaviour half of spike 2: a Crux app whose model is the current time and whose
//! view model is the string to put on screen. It knows nothing about the device, which is
//! what lets the tests below run on the laptop against the same code the shell flashes.

use chrono::{DateTime, Utc};
use crux_core::{
    App, Command,
    macros::effect,
    render::{self, RenderOperation},
};
use serde::{Deserialize, Serialize};

/// Shown before the shell has reported a time, so the view model is always renderable.
const NO_TIME_YET: &str = "--:--:--";

#[derive(Debug, Default)]
pub struct Model {
    now: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    Tick(DateTime<Utc>),
}

#[effect]
pub enum Effect {
    Render(RenderOperation),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ViewModel {
    pub clock: String,
}

#[derive(Debug, Default)]
pub struct Clock;

impl App for Clock {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: Event, model: &mut Model) -> Command<Effect, Event> {
        match event {
            Event::Tick(now) => model.now = Some(now),
        };

        render::render()
    }

    fn view(&self, model: &Model) -> Self::ViewModel {
        ViewModel {
            clock: model
                .now
                .map(|now| now.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| NO_TIME_YET.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use crux_core::Core;

    fn at(hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 29, hour, minute, second)
            .single()
            .expect("unambiguous test timestamp")
    }

    #[test]
    fn renders_a_placeholder_before_any_time_is_known() {
        let core: Core<Clock> = Core::new();

        assert_eq!(core.view().clock, NO_TIME_YET);
    }

    #[test]
    fn a_tick_renders_that_time() {
        let core: Core<Clock> = Core::new();

        core.process_event(Event::Tick(at(9, 5, 3)));

        assert_eq!(core.view().clock, "09:05:03");
    }

    #[test]
    fn each_tick_replaces_the_previous_time() {
        let core: Core<Clock> = Core::new();

        core.process_event(Event::Tick(at(9, 5, 3)));
        core.process_event(Event::Tick(at(9, 5, 4)));

        assert_eq!(core.view().clock, "09:05:04");
    }

    #[test]
    fn a_tick_asks_the_shell_to_render() {
        let core: Core<Clock> = Core::new();

        let effects = core.process_event(Event::Tick(at(9, 5, 3)));

        assert!(matches!(effects.as_slice(), [Effect::Render(_)]));
    }
}
