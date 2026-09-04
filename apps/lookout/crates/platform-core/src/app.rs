//! The core itself: sentences, voltages and ticks in, a panel out.

use chrono::{DateTime, Utc};
use crux_core::{
    App, Command,
    macros::effect,
    render::{self, RenderOperation},
};
use predictor::{CrowFlies, DEFAULT_RADIUS_METRES, Event as Observed, Parser, Predict};
use serde::{Deserialize, Serialize};

use crate::battery::Battery;
use crate::panel::{self, NEAREST_ON_SCREEN, NO_FIX_YET, NO_TIME_YET, ViewModel};
use crate::pointset::PointSet;
use crate::{Float, carried};

pub struct Model {
    now: Option<DateTime<Utc>>,
    parser: Parser<Float>,
    battery: Battery,
    predictor: CrowFlies<Float, PointSet<'static>>,
}

/// The crossings are borrowed once, not per scan: they are the same bytes for the life of the
/// binary.
impl Default for Model {
    fn default() -> Self {
        Self {
            now: None,
            parser: Parser::new(),
            battery: Battery::default(),
            predictor: CrowFlies::new(carried::crossings(), DEFAULT_RADIUS_METRES),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    /// The time, as the shell reads it. Set that clock from the receiver: the predictor
    /// refuses one behind its own fixes, leaving the countdowns to advance on fixes alone.
    Tick(DateTime<Utc>),
    /// One raw NMEA sentence, exactly as it came off the UART.
    Sentence(String),
    /// The battery terminal voltage the shell measured, in millivolts. What it means is
    /// decided here, not there — see [`crate::battery`].
    Battery(u16),
}

#[effect]
pub enum Effect {
    Render(RenderOperation),
}

/// Whether an event moved anything the panel shows, which is what decides a redraw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Moved,
    Unchanged,
}

#[derive(Debug, Default)]
pub struct Lookout;

impl Lookout {
    /// Feeds one sentence to the parser and predicts again from the fix it completes.
    ///
    /// A sentence completing nothing leaves the last fix and prediction in place: one the
    /// receiver emits before it has a fix, one failing its checksum, one repeating what is
    /// known.
    ///
    /// That matters at a dozen sentences a second. Otherwise one position would scan the
    /// whole set, and redraw the screen, a dozen times.
    fn absorb(&self, sentence: &str, model: &mut Model) -> Change {
        let Some(sample) = model.parser.absorb(sentence) else {
            return Change::Unchanged;
        };
        self.observe(Observed::Sampled(sample), model)
    }

    /// Tells the predictor. An event it refuses changes nothing there, so nothing on the
    /// panel has moved either.
    fn observe(&self, event: Observed<Float>, model: &mut Model) -> Change {
        match model.predictor.observe(event) {
            Ok(()) => Change::Moved,
            Err(_) => Change::Unchanged,
        }
    }
}

impl App for Lookout {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    /// A render is asked for only where the panel would draw something different.
    fn update(&self, event: Event, model: &mut Model) -> Command<Effect, Event> {
        let change = match event {
            // The clock is on the screen, so every tick draws something different.
            Event::Tick(now) => {
                model.now = Some(now);
                self.observe(Observed::Elapsed(now), model);
                Change::Moved
            }
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

    fn view(&self, model: &Model) -> Self::ViewModel {
        // The predictor's clock, not the shell's: the arrival instants are on it, and it
        // advances on a fix even where the shell's clock lags.
        let now = model.predictor.now();
        // The fix the predictions were made from, so the panel cannot report a position the
        // scan never ran from.
        let fix = model.predictor.latest();
        let predictions = model.predictor.predictions();

        ViewModel {
            clock: model
                .now
                .map(|now| now.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| NO_TIME_YET.to_string()),
            latitude: fix
                .map(|fix| format!("{:.5}", fix.latitude()))
                .unwrap_or_else(|| NO_FIX_YET.to_string()),
            longitude: fix
                .map(|fix| format!("{:.5}", fix.longitude()))
                .unwrap_or_default(),
            battery: model
                .battery
                .charge()
                .map(panel::bars)
                .unwrap_or_default()
                .to_string(),
            quality: match (
                fix.and_then(|fix| fix.satellites),
                fix.and_then(|fix| fix.hdop),
            ) {
                (Some(satellites), Some(hdop)) => format!("{satellites}sat h{hdop:.1}"),
                (Some(satellites), None) => format!("{satellites}sat"),
                _ => String::new(),
            },
            // Nothing about crossings until there is a fix to have scanned them from.
            within: match fix {
                None => String::new(),
                Some(_) => panel::within(predictions.len()),
            },
            nearest: predictions
                .iter()
                .take(NEAREST_ON_SCREEN)
                .map(|prediction| panel::line(prediction, now))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use crux_core::Core;
    use predictor::fixtures::{Fix, RMC_VOID};

    use super::*;
    use crate::panel::{CHARACTERS_PER_LINE, NO_ARRIVAL};

    /// Dresden Hauptbahnhof, the landmark [`crate::carried`] also checks the set against, and
    /// a place with twenty crossings inside the radius. At 54 knots, about 100 km/h, the
    /// countdowns are a train's.
    fn at_the_station() -> Fix {
        Fix::at(20, 43, 29, 51.0403, 13.7322)
            .with_speed_knots(54.0)
            .with_course_degrees(79.94)
    }

    /// A second later and a little north-east, so the fix moves and the scan runs again.
    fn moved_on() -> Fix {
        Fix::at(20, 43, 30, 51.0404, 13.73235)
            .with_speed_knots(54.0)
            .with_course_degrees(79.94)
    }

    /// Standing at the station: a speed of zero and no course, so no arrival to count down to.
    fn stopped() -> Fix {
        Fix::at(20, 48, 58, 51.0403, 13.7322)
    }

    /// When the fixtures are dated. A sentence carries the time of day and the date in
    /// separate fields, and a countdown advances only where the shell's clock agrees with
    /// both.
    fn fix_instant() -> DateTime<Utc> {
        at_the_station().t()
    }

    fn core() -> Core<Lookout> {
        Core::new()
    }

    /// A core that has seen one fix at the station, moving.
    fn fixed() -> Core<Lookout> {
        let core = core();
        core.process_event(Event::Sentence(at_the_station().rmc()));
        core.process_event(Event::Sentence(at_the_station().gga()));
        core
    }

    #[test]
    fn reports_no_fix_until_a_position_arrives() {
        let core = core();

        assert_eq!(core.view().latitude, NO_FIX_YET);
        assert_eq!(core.view().longitude, "");
        assert_eq!(core.view().clock, NO_TIME_YET);
    }

    #[test]
    fn a_sentence_carrying_a_position_produces_a_fix() {
        let core = fixed();

        assert_eq!(core.view().latitude, "51.04030");
        assert_eq!(core.view().longitude, "13.73220");
    }

    #[test]
    fn a_void_sentence_leaves_the_last_fix_alone() {
        let core = fixed();

        core.process_event(Event::Sentence(RMC_VOID.to_string()));

        assert_eq!(core.view().latitude, "51.04030");
    }

    #[test]
    fn noise_on_the_line_does_not_panic() {
        let core = core();

        core.process_event(Event::Sentence("\0\u{1}not a sentence".to_string()));

        assert_eq!(core.view().latitude, NO_FIX_YET);
    }

    #[test]
    fn a_sentence_asks_the_shell_to_render() {
        let core = core();

        let effects = core.process_event(Event::Sentence(at_the_station().rmc()));

        assert!(matches!(effects.as_slice(), [Effect::Render(_)]));
    }

    /// A dozen sentences a second arrive and most repeat the position of the one before. A
    /// render for each would spend a second of screen redrawing one second of fixes.
    #[test]
    fn a_sentence_that_changes_nothing_asks_for_no_render() {
        let core = fixed();

        let effects = core.process_event(Event::Sentence(at_the_station().rmc()));

        assert!(effects.is_empty());
    }

    /// The reading is taken on a timer, and the bars it fills change a handful of times over a
    /// whole discharge.
    #[test]
    fn a_battery_reading_that_fills_the_same_bars_asks_for_no_render() {
        let core = core();

        assert!(!core.process_event(Event::Battery(4_200)).is_empty());
        assert!(core.process_event(Event::Battery(4_190)).is_empty());
    }

    #[test]
    fn the_clock_shows_what_the_shell_last_reported() {
        let core = core();

        core.process_event(Event::Tick(fix_instant()));

        assert_eq!(core.view().clock, "20:43:29");
    }

    /// Both are needed to read a jittering distance: 8 satellites at HDOP 2.4 wanders about a
    /// metre, 6 at HDOP 4.4 wanders metres a second and lies about its speed too.
    #[test]
    fn the_fix_reports_how_good_it_is() {
        let core = fixed();

        assert_eq!(core.view().quality, "6sat h4.4");
    }

    #[test]
    fn nothing_is_said_about_crossings_until_there_is_a_fix() {
        let core = core();

        assert!(core.view().nearest.is_empty());
        assert_eq!(core.view().within, "");
    }

    #[test]
    fn a_fix_fills_the_screen_with_the_nearest_crossings() {
        let core = fixed();

        assert_eq!(core.view().nearest.len(), NEAREST_ON_SCREEN);
    }

    /// The count beside the fix and the list under it come from one set of predictions, so
    /// the screen cannot show a crossing 300 m away and claim none is near.
    #[test]
    fn the_count_and_the_list_agree() {
        let core = fixed();
        let view = core.view();

        assert_eq!(view.within, "20 in 5km");
        assert!(view.nearest.len() <= 20);
    }

    /// The prediction itself, on a line: the nearest crossing to the station is 2.3 km away,
    /// and at 100 km/h we reach it in a minute and a half.
    #[test]
    fn a_line_says_how_far_and_how_long() {
        let core = fixed();

        assert_eq!(core.view().nearest[0], "2.3km 1:24");
    }

    /// The countdown is a clock subtracted from an instant, so it shortens between fixes
    /// rather than sitting at whatever the last fix said.
    #[test]
    fn a_countdown_shortens_as_time_passes() {
        let core = fixed();

        core.process_event(Event::Tick(fix_instant() + TimeDelta::seconds(30)));

        assert_eq!(core.view().nearest[0], "2.3km 0:54");
    }

    #[test]
    fn a_fix_that_is_not_moving_predicts_a_distance_and_no_time() {
        let core = core();

        core.process_event(Event::Sentence(stopped().rmc()));

        assert_eq!(core.view().nearest[0], format!("2.3km {NO_ARRIVAL}"));
    }

    #[test]
    fn moving_changes_what_is_predicted() {
        let core = fixed();
        let before = core.view().nearest;

        core.process_event(Event::Sentence(moved_on().rmc()));

        assert_ne!(core.view().nearest, before);
    }

    /// Scanning the whole set again for a position already scanned would waste a second the
    /// device does not have.
    #[test]
    fn the_predictions_do_not_change_while_the_fix_does_not() {
        let core = fixed();
        let first = core.view().nearest;

        core.process_event(Event::Sentence(at_the_station().rmc()));

        assert_eq!(core.view().nearest, first);
    }

    /// The panel says nothing about the battery until a plausible reading arrives, rather
    /// than showing an empty meter it has no grounds for.
    #[test]
    fn the_battery_is_blank_until_it_is_measured() {
        let core = core();

        assert_eq!(core.view().battery, "");
    }

    #[test]
    fn a_measured_battery_fills_its_bars() {
        let core = core();

        core.process_event(Event::Battery(4_200));
        assert_eq!(core.view().battery, "[===]");

        core.process_event(Event::Battery(3_200));
        assert_eq!(core.view().battery, "[   ]");
    }

    #[test]
    fn every_line_fits_the_panel() {
        let core = fixed();
        core.process_event(Event::Tick(fix_instant()));
        core.process_event(Event::Battery(4_200));
        let view = core.view();

        for line in [
            &view.clock,
            &view.latitude,
            &view.longitude,
            &view.quality,
            &view.within,
        ]
        .into_iter()
        .chain(view.nearest.iter())
        {
            assert!(
                line.chars().count() <= CHARACTERS_PER_LINE,
                "{line:?} is {} characters, more than a line holds",
                line.chars().count(),
            );
        }
    }
}
