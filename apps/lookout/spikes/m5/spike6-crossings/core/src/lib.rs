//! Spike 6's behaviour: accumulate NMEA sentences into a position fix, and report the
//! crossings nearest to it. Both the parsing and the scan live here rather than in the
//! shell so they can be tested on the laptop — the GPS needs sky view and a ~23s cold
//! start, which makes on-device iteration slow.

pub mod carried;
pub mod pointset;
pub mod scan;

use chrono::{DateTime, NaiveTime, Utc};
use crux_core::{
    App, Command,
    macros::effect,
    render::{self, RenderOperation},
};
use geo::Point;
use nmea::Nmea;
use serde::{Deserialize, Serialize};

use crate::scan::{Near, Nearby};

/// Shown before the shell has reported a time, so the view model is always renderable.
const NO_TIME_YET: &str = "--:--:--";
/// Shown while the receiver has yet to produce a position.
const NO_FIX_YET: &str = "no fix";
/// Shown if the crossings built into the binary cannot be read, which no amount of waiting
/// will fix — the bytes are the same on every boot.
const NO_CROSSINGS: &str = "no set";

/// How many crossings the panel has room for beneath the fix: five 20-pixel lines, ending
/// clear of the bottom of a 240-pixel screen.
pub const NEAREST_ON_SCREEN: usize = 5;
/// The radius the predictor's half of the scan reports on. Wide enough that a train at speed
/// has a minute or two of warning, narrow enough to mean something at walking pace.
pub const WITHIN_METRES: f32 = 5_000.0;
/// A line is 13 characters at 10 pixels each on a 135-pixel panel, and an id takes 6 of them,
/// leaving 6 for a distance and one for the space between.
const ID_CHARACTERS: usize = 6;

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
    /// What the last scan found. `None` until there is a fix to scan from, and also if the
    /// crossings built into the binary could not be read.
    nearby: Option<Nearby>,
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
    /// Satellites and HDOP. On a train the panel is the only output — the serial console
    /// needs a laptop — and without this there is no way to tell a jittering distance from a
    /// jittering fix.
    pub quality: String,
    /// How many crossings are inside [`WITHIN_METRES`] — the predictor's half of the scan,
    /// shown so that both halves are visible on the panel.
    pub within: String,
    /// The nearest crossings, nearest first, each already formatted to fit a line.
    pub nearest: Vec<String>,
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

        let (Some(latitude), Some(longitude)) =
            (model.sentences.latitude, model.sentences.longitude)
        else {
            return;
        };

        if let (Ok(latitude), Ok(longitude)) = (Latitude::new(latitude), Longitude::new(longitude))
        {
            let fix = GnssFix {
                latitude,
                longitude,
                at: model.sentences.fix_time,
            };
            let moved = model
                .fix
                .map(|last| last.latitude != fix.latitude || last.longitude != fix.longitude);
            model.fix = Some(fix);

            // Sentences arrive a dozen a second and most repeat the same position, so the
            // scan runs when the position changes rather than when a sentence lands.
            if moved != Some(false) {
                self.rescan(model);
            }
        }
    }

    /// Scans the crossings carried in flash against the current fix.
    fn rescan(&self, model: &mut Model) {
        let Some(fix) = model.fix else {
            return;
        };
        model.nearby = carried::crossings().ok().map(|crossings| {
            scan::nearby(
                &crossings,
                Point::new(
                    fix.longitude.degrees() as f32,
                    fix.latitude.degrees() as f32,
                ),
                NEAREST_ON_SCREEN,
                WITHIN_METRES,
            )
        });
    }
}

/// A crossing on one line of the panel: an abbreviated id, then how far away it is.
///
/// The id is cut to its first [`ID_CHARACTERS`] because the line has room for no more, and
/// telling apart the handful on screen is all it has to do here.
fn line(near: &Near) -> String {
    let id = format!("{:08x}", near.crossing.id);
    format!("{} {}", &id[..ID_CHARACTERS], distance(near.metres))
}

/// A distance in as few characters as it can be said in, never more than six: metres up to a
/// kilometre, then kilometres, and past a thousand of those only that it is a long way.
fn distance(metres: f32) -> String {
    match metres {
        metres if metres < 1_000.0 => format!("{metres:.0}m"),
        metres if metres < 100_000.0 => format!("{:.1}km", metres / 1_000.0),
        metres if metres < 1_000_000.0 => format!("{:.0}km", metres / 1_000.0),
        _ => ">999km".to_string(),
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
            quality: match (model.sentences.num_of_fix_satellites, model.sentences.hdop) {
                (Some(satellites), Some(hdop)) => format!("{satellites}sat h{hdop:.1}"),
                (Some(satellites), None) => format!("{satellites}sat"),
                _ => String::new(),
            },
            within: match (&model.fix, &model.nearby) {
                (None, _) => String::new(),
                (Some(_), None) => NO_CROSSINGS.to_string(),
                (Some(_), Some(nearby)) => format!(
                    "{} in {:.0}km",
                    nearby.within.len(),
                    WITHIN_METRES / 1_000.0
                ),
            },
            nearest: model
                .nearby
                .as_ref()
                .map(|nearby| nearby.nearest.iter().map(line).collect())
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
    const GGA_FIX: &str = "GNGGA,204329.00,5030.00000,N,00830.00000,E,1,06,4.4,262.46,M,45.12,M,,";
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

    /// What the panel can show: 13 characters of a 10-pixel font across 135 pixels. A longer
    /// line does not wrap, it runs off the side.
    const CHARACTERS_PER_LINE: usize = 13;

    /// Both are needed to read a jittering distance: 8 satellites at HDOP 2.4 wanders about a
    /// metre, 6 at HDOP 4.4 wanders metres a second and lies about its speed too.
    #[test]
    fn the_fix_reports_how_good_it_is() {
        let core = core();

        core.process_event(Event::Sentence(sentence(GGA_FIX)));

        assert_eq!(core.view().quality, "6sat h4.4");
    }

    /// The widest it can get is still a line: two digits of satellites and a two-digit HDOP.
    #[test]
    fn the_quality_line_fits_at_its_widest() {
        let widest = format!("{}sat h{:.1}", 12, 99.9);

        assert!(widest.chars().count() <= CHARACTERS_PER_LINE, "{widest:?}");
    }

    #[test]
    fn nothing_is_said_about_crossings_until_there_is_a_fix() {
        let core = core();

        assert!(core.view().nearest.is_empty());
        assert_eq!(core.view().within, "");
    }

    #[test]
    fn a_fix_fills_the_screen_with_the_nearest_crossings() {
        let core = core();

        core.process_event(Event::Sentence(sentence(GGA_FIX)));

        assert_eq!(core.view().nearest.len(), NEAREST_ON_SCREEN);
    }

    /// The whole point of one pass: the count beside the fix and the list under it are the
    /// same distances, so the screen cannot show a crossing 300m away and claim none is near.
    #[test]
    fn the_count_and_the_list_agree() {
        let core = core();

        core.process_event(Event::Sentence(sentence(GGA_FIX)));
        let view = core.view();

        let within: usize = view
            .within
            .split_whitespace()
            .next()
            .expect("a count")
            .parse()
            .expect("a number");
        let listed_within = view
            .nearest
            .iter()
            .filter(|line| line.ends_with('m') && !line.ends_with("km"))
            .count();
        assert!(
            listed_within <= within,
            "{listed_within} listed under a kilometre, but only {within} claimed within 5km",
        );
    }

    #[test]
    fn every_line_fits_the_panel() {
        let core = core();

        core.process_event(Event::Sentence(sentence(GGA_FIX)));
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
                "{line:?} is {} characters, more than {CHARACTERS_PER_LINE}",
                line.chars().count(),
            );
        }
    }

    /// Including the widest a line can get: a long id and a distance in every band it has a
    /// format for.
    #[test]
    fn no_distance_can_make_a_line_too_long() {
        for metres in [
            0.0,
            999.4,
            999.6,
            1_000.0,
            99_949.0,
            100_000.0,
            999_999.0,
            20_015_000.0,
        ] {
            let line = line(&Near {
                crossing: crate::pointset::Point {
                    id: u32::MAX,
                    latitude: 51.5,
                    longitude: 13.5,
                },
                metres,
            });
            assert!(
                line.chars().count() <= CHARACTERS_PER_LINE,
                "{line:?} is {} characters at {metres}m",
                line.chars().count(),
            );
        }
    }

    #[test]
    fn a_distance_is_said_in_the_unit_that_suits_it() {
        assert_eq!(distance(0.0), "0m");
        assert_eq!(distance(942.0), "942m");
        assert_eq!(distance(1_500.0), "1.5km");
        assert_eq!(distance(250_000.0), "250km");
        assert_eq!(distance(20_015_000.0), ">999km");
    }

    #[test]
    fn a_line_names_the_crossing_it_reports() {
        let near = Near {
            crossing: crate::pointset::Point {
                id: 0x292e_417a,
                latitude: 51.5,
                longitude: 13.5,
            },
            metres: 1_500.0,
        };

        assert_eq!(line(&near), "292e41 1.5km");
    }

    /// A dozen sentences a second arrive carrying the same position, and scanning 5,749
    /// points for each of them would be a waste of a second the device does not have.
    #[test]
    fn the_nearest_do_not_change_while_the_fix_does_not() {
        let core = core();

        core.process_event(Event::Sentence(sentence(GGA_FIX)));
        let first = core.view().nearest;
        core.process_event(Event::Sentence(sentence(RMC_FIX)));

        assert_eq!(core.view().nearest, first);
    }

    #[test]
    fn moving_changes_what_is_nearest() {
        let core = core();

        core.process_event(Event::Sentence(sentence(GGA_FIX)));
        let before = core.view().nearest;
        core.process_event(Event::Sentence(sentence(GGA_LATER)));

        assert_ne!(core.view().nearest, before);
    }
}
