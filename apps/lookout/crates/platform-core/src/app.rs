//! The core itself: sentences, voltages and ticks in, a panel out.

use chrono::{DateTime, Utc};
use crux_core::{
    App, Command,
    macros::effect,
    render::{self, RenderOperation},
};
use predictor::{CrowFlies, Event as Observed, Parser, Predict, Sample};
use serde::{Deserialize, Serialize};

use crate::battery::Battery;
use crate::panel::{self, NEAREST_ON_SCREEN, NO_FIX_YET, NO_TIME_YET, ViewModel};
use crate::pointset::PointSet;
use crate::{Float, WITHIN_METRES, carried};

pub struct Model {
    now: Option<DateTime<Utc>>,
    /// Sentences arrive one at a time and each fills in part of the picture, so the parser
    /// keeps state across them.
    parser: Parser<Float>,
    /// The last fix, which is what the panel reports and what the predictor last saw.
    fix: Option<Sample<Float>>,
    battery: Battery,
    predictor: CrowFlies<Float, PointSet<'static>>,
}

/// The crossings are borrowed once rather than on every scan: they are the same bytes for the
/// life of the binary.
impl Default for Model {
    fn default() -> Self {
        Self {
            now: None,
            parser: Parser::new(),
            fix: None,
            battery: Battery::default(),
            predictor: CrowFlies::new(carried::crossings(), WITHIN_METRES),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    /// The time, as the shell reads it. Set the shell's clock from the receiver: a clock
    /// behind the fixes is refused by the predictor, which leaves the countdowns to advance
    /// on fixes alone.
    Tick(DateTime<Utc>),
    /// One raw NMEA sentence, exactly as it came off the UART.
    Sentence(String),
    /// The battery terminal voltage the shell measured, in millivolts. What it *means* is
    /// decided here, not there — see [`crate::battery`].
    Battery(u16),
}

#[effect]
pub enum Effect {
    Render(RenderOperation),
}

#[derive(Debug, Default)]
pub struct Lookout;

impl Lookout {
    /// Feeds one sentence to the parser and predicts again from the fix it completes.
    ///
    /// Sentences the receiver emits before it has a fix, ones that fail their checksum, and
    /// ones repeating what is already known all complete nothing, and leave the last fix and
    /// the last prediction in place. That matters at a dozen sentences a second: without it
    /// the whole set would be scanned a dozen times for one position.
    fn absorb(&self, sentence: &str, model: &mut Model) {
        let Some(sample) = model.parser.absorb(sentence) else {
            return;
        };
        model.fix = Some(sample);
        self.observe(Observed::Sampled(sample), model);
    }

    /// Tells the predictor, ignoring what it refuses. An event out of order changes nothing
    /// there, and there is nothing the panel can do about one.
    fn observe(&self, event: Observed<Float>, model: &mut Model) {
        let _ = model.predictor.observe(event);
    }
}

impl App for Lookout {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: Event, model: &mut Model) -> Command<Effect, Event> {
        match event {
            Event::Tick(now) => {
                model.now = Some(now);
                self.observe(Observed::Elapsed(now), model);
            }
            Event::Sentence(sentence) => self.absorb(&sentence, model),
            Event::Battery(millivolts) => model.battery.measured(millivolts),
        };

        render::render()
    }

    fn view(&self, model: &Model) -> Self::ViewModel {
        // The predictor's clock, not the shell's: it is the one the arrival instants are on,
        // and it advances on a fix even where the shell's clock is behind them.
        let now = model.predictor.now();
        let predictions = model.predictor.predictions();

        ViewModel {
            clock: model
                .now
                .map(|now| now.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| NO_TIME_YET.to_string()),
            latitude: model
                .fix
                .map(|fix| format!("{:.5}", fix.latitude()))
                .unwrap_or_else(|| NO_FIX_YET.to_string()),
            longitude: model
                .fix
                .map(|fix| format!("{:.5}", fix.longitude()))
                .unwrap_or_default(),
            battery: model.battery.charge().map(panel::bars).unwrap_or_default(),
            quality: match (
                model.fix.and_then(|fix| fix.satellites),
                model.fix.and_then(|fix| fix.hdop),
            ) {
                (Some(satellites), Some(hdop)) => format!("{satellites}sat h{hdop:.1}"),
                (Some(satellites), None) => format!("{satellites}sat"),
                _ => String::new(),
            },
            // Nothing about crossings until there is a fix to have scanned them from.
            within: match model.fix {
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
    use chrono::{TimeDelta, TimeZone};
    use crux_core::Core;

    use super::*;
    use crate::panel::{CHARACTERS_PER_LINE, NO_ARRIVAL};

    /// Captured from the AT6668 indoors, so this is the real thing: no fix yet, but the
    /// actual sentence shape the receiver emits.
    const RMC_VOID: &str = "$GNRMC,202725.00,V,,,,,,,290726,,,N,V*11";

    /// Bodies of sentences in the shape the AT6668 emits them, down to the field count and
    /// the NMEA 4.1 mode/status pair at the end of `RMC`, carrying **Dresden Hauptbahnhof**
    /// — the same public landmark [`crate::carried`] checks the crossings set against, and a
    /// place with twenty crossings inside the radius to predict. [`sentence`] appends the
    /// checksum.
    ///
    /// The speed is 54 knots, about 100 km/h, so the countdowns are a train's.
    const RMC_FIX: &str = "GNRMC,204329.00,A,5102.41800,N,01343.93200,E,54.00,79.94,290726,,,A,V";
    const GGA_FIX: &str = "GNGGA,204329.00,5102.41800,N,01343.93200,E,1,06,4.4,262.46,M,45.12,M,,";
    /// A second later and a little north-east, so the fix moves and the scan runs again.
    const RMC_LATER: &str = "GNRMC,204330.00,A,5102.42400,N,01343.94100,E,54.00,79.94,290726,,,A,V";
    /// A receiver standing still reports a speed of zero and no course at all — the `0.00,,`
    /// here. There is then no arrival to count down to.
    const RMC_STOPPED: &str = "GNRMC,204858.00,A,5102.41800,N,01343.93200,E,0.00,,290726,,,A,V";

    /// When the fixtures above are dated. A sentence carries the time of day and the date in
    /// separate fields, and a countdown only advances if the shell's clock agrees with both.
    fn fix_instant() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 29, 20, 43, 29)
            .single()
            .expect("an instant")
    }

    /// Wraps a sentence body into the on-the-wire form: `$`, the body, then `*` and the XOR
    /// of every body byte as two hex digits.
    fn sentence(body: &str) -> String {
        let checksum = body.bytes().fold(0u8, |acc, byte| acc ^ byte);
        format!("${body}*{checksum:02X}")
    }

    fn core() -> Core<Lookout> {
        Core::new()
    }

    /// A core that has seen one fix at the station, moving.
    fn fixed() -> Core<Lookout> {
        let core = core();
        core.process_event(Event::Sentence(sentence(RMC_FIX)));
        core.process_event(Event::Sentence(sentence(GGA_FIX)));
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

        let effects = core.process_event(Event::Sentence(sentence(RMC_FIX)));

        assert!(matches!(effects.as_slice(), [Effect::Render(_)]));
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

        core.process_event(Event::Sentence(sentence(RMC_STOPPED)));

        assert_eq!(core.view().nearest[0], format!("2.3km {NO_ARRIVAL}"));
    }

    #[test]
    fn moving_changes_what_is_predicted() {
        let core = fixed();
        let before = core.view().nearest;

        core.process_event(Event::Sentence(sentence(RMC_LATER)));

        assert_ne!(core.view().nearest, before);
    }

    /// A dozen sentences a second arrive carrying the same position, and scanning the whole
    /// set for each of them would be a waste of a second the device does not have.
    #[test]
    fn the_predictions_do_not_change_while_the_fix_does_not() {
        let core = fixed();
        let first = core.view().nearest;

        core.process_event(Event::Sentence(sentence(RMC_FIX)));

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
