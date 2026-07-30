//! Spike 5's behaviour: accumulate NMEA sentences into a position fix, and report the
//! crossings nearest to it. Both the parsing and the scan live here rather than in the
//! shell so they can be tested on the laptop — the GPS needs sky view and a ~23s cold
//! start, which makes on-device iteration slow.

use chrono::{DateTime, NaiveTime, Utc};
use crux_core::{
    App, Command,
    macros::effect,
    render::{self, RenderOperation},
};
use nmea::Nmea;
use serde::{Deserialize, Serialize};

/// Shown before the shell has reported a time, so the view model is always renderable.
const NO_TIME_YET: &str = "--:--:--";
/// Shown while the receiver has yet to produce a position.
const NO_FIX_YET: &str = "no fix";

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum CoordinateError {
    #[error("latitude {0} outside -90..=90")]
    Latitude(f64),
    #[error("longitude {0} outside -180..=180")]
    Longitude(f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Latitude(f64);

impl Latitude {
    pub fn new(degrees: f64) -> Result<Self, CoordinateError> {
        (-90.0..=90.0)
            .contains(&degrees)
            .then_some(Self(degrees))
            .ok_or(CoordinateError::Latitude(degrees))
    }

    pub fn degrees(&self) -> f64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Longitude(f64);

impl Longitude {
    pub fn new(degrees: f64) -> Result<Self, CoordinateError> {
        (-180.0..=180.0)
            .contains(&degrees)
            .then_some(Self(degrees))
            .ok_or(CoordinateError::Longitude(degrees))
    }

    pub fn degrees(&self) -> f64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GnssFix {
    pub latitude: Latitude,
    pub longitude: Longitude,
    pub at: Option<NaiveTime>,
}

#[derive(Default)]
pub struct Model {
    now: Option<DateTime<Utc>>,
    /// The `nmea` crate's own accumulator: sentences arrive one at a time and each fills
    /// in part of the picture, so the parser has to keep state across them.
    sentences: Nmea,
    fix: Option<GnssFix>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    Tick(DateTime<Utc>),
    /// One raw NMEA sentence, exactly as it came off the UART.
    Sentence(String),
}

#[effect]
pub enum Effect {
    Render(RenderOperation),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ViewModel {
    pub clock: String,
    pub latitude: String,
    pub longitude: String,
}

#[derive(Debug, Default)]
pub struct Gnss;

impl Gnss {
    /// Feeds one sentence to the accumulator and promotes it to a fix once both a latitude
    /// and a longitude are known. Sentences the receiver emits before it has a fix, and
    /// ones that fail their checksum, simply leave the last known fix in place.
    fn absorb(&self, sentence: &str, model: &mut Model) {
        if model.sentences.parse(sentence).is_err() {
            return;
        }

        let (Some(latitude), Some(longitude)) = (model.sentences.latitude, model.sentences.longitude)
        else {
            return;
        };

        if let (Ok(latitude), Ok(longitude)) = (Latitude::new(latitude), Longitude::new(longitude)) {
            model.fix = Some(GnssFix {
                latitude,
                longitude,
                at: model.sentences.fix_time,
            });
        }
    }
}

impl App for Gnss {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;
    /// Unused: side-effects are described by the returned `Command`. The associated type is
    /// required by this crux version and was dropped in later ones.
    type Capabilities = ();

    fn update(&self, event: Event, model: &mut Model, _caps: &()) -> Command<Effect, Event> {
        match event {
            Event::Tick(now) => model.now = Some(now),
            Event::Sentence(sentence) => self.absorb(&sentence, model),
        };

        render::render()
    }

    fn view(&self, model: &Model) -> Self::ViewModel {
        ViewModel {
            clock: model
                .now
                .map(|now| now.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| NO_TIME_YET.to_string()),
            latitude: model
                .fix
                .map(|fix| format!("{:.5}", fix.latitude.degrees()))
                .unwrap_or_else(|| NO_FIX_YET.to_string()),
            longitude: model
                .fix
                .map(|fix| format!("{:.5}", fix.longitude.degrees()))
                .unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crux_core::Core;

    /// Captured from the AT6668 indoors, so these are the real thing: no fix yet, but the
    /// actual sentence shapes the receiver emits. Note `RMC_VOID` carries the NMEA 4.1
    /// navigational-status field (the trailing `,V`) that a hand-written 0183 RMC lacks.
    const RMC_VOID: &str = "$GNRMC,202725.00,V,,,,,,,290726,,,N,V*11";
    const GGA_NO_FIX: &str = "$GNGGA,202725.00,,,,,0,00,25.5,,,,,,*4A";
    const GSA_NO_FIX: &str = "$GNGSA,A,1,,,,,,,,,,,,,25.5,25.5,25.5,1*01";
    /// Also captured: an RX overrun spliced two sentences together. Worth keeping as a test
    /// even though the shell's larger ring buffer should now prevent it — corruption on a
    /// serial line is never fully ruled out.
    const SPLICED: &str = "$GAGSV,12724.00,V,N*55";

    /// Bodies of sentences taken from a real outdoor capture, with **the position replaced**
    /// — a real fix pins down where and when someone was, which is not something to commit.
    /// Everything else is as the AT6668 emitted it, including the field count and the NMEA
    /// 4.1 mode/status pair at the end of `RMC`. `sentence` appends the checksum, since
    /// changing the coordinates invalidates the captured one.
    ///
    /// They encode a fix at 50.5N 8.5E and a second slightly north-east of it.
    const RMC_FIX: &str = "GNRMC,204329.00,A,5030.00000,N,00830.00000,E,4.13,79.94,290726,,,A,V";
    const GGA_FIX: &str =
        "GNGGA,204329.00,5030.00000,N,00830.00000,E,1,06,4.4,262.46,M,45.12,M,,";
    const GGA_LATER: &str =
        "GNGGA,204330.00,5030.00600,N,00830.00900,E,1,06,4.4,262.46,M,45.12,M,,";
    /// A stationary receiver leaves the course field **empty** (the `0.08,,` here) because
    /// there is no meaningful heading. Captured; a fix still has to come out of it.
    const RMC_STATIONARY: &str = "GNRMC,204858.00,A,5030.00000,N,00830.00000,E,0.08,,290726,,,A,V";

    /// Wraps a sentence body into the on-the-wire form: `$`, the body, then `*` and the XOR
    /// of every body byte as two hex digits.
    fn sentence(body: &str) -> String {
        format!("${body}*{:02X}", checksum(body))
    }

    /// The same sentence with a checksum that is guaranteed wrong — inverting every bit
    /// cannot land back on the correct value, which fabricating one by hand might.
    fn sentence_with_bad_checksum(body: &str) -> String {
        format!("${body}*{:02X}", checksum(body) ^ 0xFF)
    }

    fn checksum(body: &str) -> u8 {
        body.bytes().fold(0u8, |acc, byte| acc ^ byte)
    }

    fn core() -> Core<Gnss> {
        Core::new()
    }

    #[test]
    fn reports_no_fix_until_a_position_arrives() {
        let core = core();

        assert_eq!(core.view().latitude, NO_FIX_YET);
        assert_eq!(core.view().longitude, "");
    }

    #[test]
    fn a_gga_sentence_produces_a_fix() {
        let core = core();

        core.process_event(Event::Sentence(sentence(GGA_FIX)));

        assert_eq!(core.view().latitude, "50.50000");
        assert_eq!(core.view().longitude, "8.50000");
    }

    #[test]
    fn an_rmc_sentence_produces_a_fix() {
        let core = core();

        core.process_event(Event::Sentence(sentence(RMC_FIX)));

        assert_eq!(core.view().latitude, "50.50000");
        assert_eq!(core.view().longitude, "8.50000");
    }

    #[test]
    fn a_stationary_rmc_with_no_course_still_produces_a_fix() {
        let core = core();

        core.process_event(Event::Sentence(sentence(RMC_STATIONARY)));

        assert_eq!(core.view().latitude, "50.50000");
        assert_eq!(core.view().longitude, "8.50000");
    }

    #[test]
    fn a_later_sentence_moves_the_fix() {
        let core = core();

        core.process_event(Event::Sentence(sentence(GGA_FIX)));
        core.process_event(Event::Sentence(sentence(GGA_LATER)));

        assert_eq!(core.view().latitude, "50.50010");
        assert_eq!(core.view().longitude, "8.50015");
    }

    #[test]
    fn a_void_sentence_leaves_the_last_fix_alone() {
        let core = core();

        core.process_event(Event::Sentence(sentence(GGA_FIX)));
        core.process_event(Event::Sentence(RMC_VOID.to_string()));

        assert_eq!(core.view().latitude, "50.50000");
    }

    #[test]
    fn the_real_indoor_stream_reports_no_fix() {
        let core = core();

        for sentence in [RMC_VOID, GGA_NO_FIX, GSA_NO_FIX, SPLICED] {
            core.process_event(Event::Sentence(sentence.to_string()));
        }

        assert_eq!(core.view().latitude, NO_FIX_YET);
        assert_eq!(core.view().longitude, "");
    }

    #[test]
    fn a_spliced_sentence_leaves_the_last_fix_alone() {
        let core = core();

        core.process_event(Event::Sentence(sentence(GGA_FIX)));
        core.process_event(Event::Sentence(SPLICED.to_string()));

        assert_eq!(core.view().latitude, "50.50000");
    }

    #[test]
    fn a_corrupt_sentence_is_ignored() {
        let core = core();

        core.process_event(Event::Sentence(sentence(GGA_FIX)));
        // A well-formed later sentence whose checksum does not match its contents, so the
        // move to 50.50010 must not be believed.
        core.process_event(Event::Sentence(sentence_with_bad_checksum(GGA_LATER)));

        assert_eq!(core.view().latitude, "50.50000");
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

        let effects = core.process_event(Event::Sentence(sentence(GGA_FIX)));

        assert!(matches!(effects.as_slice(), [Effect::Render(_)]));
    }

    #[test]
    fn coordinates_outside_the_valid_range_are_rejected() {
        assert!(Latitude::new(91.0).is_err());
        assert!(Longitude::new(-181.0).is_err());
        assert!(Latitude::new(50.5).is_ok());
    }
}
