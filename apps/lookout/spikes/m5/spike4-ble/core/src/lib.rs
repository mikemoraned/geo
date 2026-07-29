//! Spike 4's behaviour: spike 3's GNSS core, plus a second effect that asks the shell to
//! publish each new fix over BLE. Both the wire format and the decision of *when* to publish
//! live here, so they are testable on the laptop; the shell only pushes bytes.

use chrono::{DateTime, NaiveTime, Utc};
use crux_core::{
    App, Command,
    capability::Operation,
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

/// Asks the shell to publish `payload` to any subscribed BLE client. The core formats the
/// bytes so the wire format is decided — and asserted — on the laptop rather than on device.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BroadcastOperation {
    pub payload: String,
}

impl Operation for BroadcastOperation {
    type Output = ();
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
    Broadcast(BroadcastOperation),
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

/// The published form of a fix: `latitude,longitude` in decimal degrees as UTF-8, so a
/// generic BLE explorer shows something legible without needing a decoder.
fn payload_for(fix: &GnssFix) -> String {
    format!(
        "{:.5},{:.5}",
        fix.latitude.degrees(),
        fix.longitude.degrees()
    )
}

fn broadcast(fix: &GnssFix) -> Command<Effect, Event> {
    Command::notify_shell(BroadcastOperation {
        payload: payload_for(fix),
    })
    .into()
}

impl App for Gnss {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: Event, model: &mut Model) -> Command<Effect, Event> {
        match event {
            Event::Tick(now) => {
                model.now = Some(now);
                render::render()
            }
            Event::Sentence(sentence) => {
                let known = model.fix;
                self.absorb(&sentence, model);

                // Publishing only on change keeps the radio quiet: the receiver sends a
                // dozen-plus sentences a second and most repeat the position already sent.
                match model.fix {
                    Some(fix) if Some(fix) != known => render::render().and(broadcast(&fix)),
                    _ => render::render(),
                }
            }
        }
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

    /// Bodies of sentences taken from a real outdoor capture, with **the position replaced**
    /// — a real fix pins down where and when someone was, which is not something to commit.
    /// `sentence` appends the checksum, since changing the coordinates invalidates the
    /// captured one. They encode a fix at 50.5N 8.5E and a second slightly north-east of it.
    const GGA_FIX: &str = "GNGGA,204329.00,5030.00000,N,00830.00000,E,1,06,4.4,262.46,M,45.12,M,,";
    const GGA_LATER: &str =
        "GNGGA,204330.00,5030.00600,N,00830.00900,E,1,06,4.4,262.46,M,45.12,M,,";

    fn sentence(body: &str) -> String {
        let checksum = body.bytes().fold(0u8, |acc, byte| acc ^ byte);

        format!("${body}*{checksum:02X}")
    }

    fn core() -> Core<Gnss> {
        Core::new()
    }

    /// The payloads the core asked to be published, ignoring render effects, so tests don't
    /// depend on the order effects come back in.
    fn published(effects: Vec<Effect>) -> Vec<String> {
        effects
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::Broadcast(request) => Some(request.operation.payload),
                Effect::Render(_) => None,
            })
            .collect()
    }

    #[test]
    fn a_new_fix_is_published() {
        let core = core();

        let effects = core.process_event(Event::Sentence(sentence(GGA_FIX)));

        assert_eq!(published(effects), ["50.50000,8.50000"]);
    }

    #[test]
    fn a_new_fix_also_renders() {
        let core = core();

        let effects = core.process_event(Event::Sentence(sentence(GGA_FIX)));

        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Render(_)))
        );
    }

    #[test]
    fn an_unchanged_position_is_not_republished() {
        let core = core();

        core.process_event(Event::Sentence(sentence(GGA_FIX)));
        let effects = core.process_event(Event::Sentence(sentence(GGA_FIX)));

        assert!(published(effects).is_empty());
    }

    #[test]
    fn a_moved_position_is_published_again() {
        let core = core();

        core.process_event(Event::Sentence(sentence(GGA_FIX)));
        let effects = core.process_event(Event::Sentence(sentence(GGA_LATER)));

        assert_eq!(published(effects), ["50.50010,8.50015"]);
    }

    #[test]
    fn sentences_without_a_fix_publish_nothing() {
        let core = core();

        let effects: Vec<Effect> = [RMC_VOID, GGA_NO_FIX]
            .into_iter()
            .flat_map(|sentence| core.process_event(Event::Sentence(sentence.to_string())))
            .collect();

        assert!(published(effects).is_empty());
    }

    #[test]
    fn a_clock_tick_publishes_nothing() {
        let core = core();

        core.process_event(Event::Sentence(sentence(GGA_FIX)));
        let effects =
            core.process_event(Event::Tick(DateTime::from_timestamp(0, 0).expect("epoch")));

        assert!(published(effects).is_empty());
    }

    #[test]
    fn the_view_still_reports_the_fix() {
        let core = core();

        core.process_event(Event::Sentence(sentence(GGA_FIX)));

        assert_eq!(core.view().latitude, "50.50000");
        assert_eq!(core.view().longitude, "8.50000");
    }

    #[test]
    fn reports_no_fix_until_a_position_arrives() {
        let core = core();

        assert_eq!(core.view().latitude, NO_FIX_YET);
        assert_eq!(core.view().longitude, "");
    }
}
