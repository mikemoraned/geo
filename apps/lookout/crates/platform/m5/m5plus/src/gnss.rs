//! The GPS/BDS Unit on the Grove port, as a source of sentences.
//!
//! Reading it is all this does. What a sentence means is the core's business, so a line shaped
//! like NMEA goes straight on as a [`Sentence`] and one that is not is dropped here.

use esp_idf_svc::hal::{
    delay::TickType,
    uart::{UartRxDriver, config::Config},
    units::FromValueType,
};
use predictor::Sentence;

/// The GPS/BDS Unit v1.1 (AT6668) talks NMEA 0183 at 115200 8N1.
pub const BAUDRATE: u32 = 115_200;

/// The default RX ring buffer is `UART_FIFO_SIZE * 2`, 256 bytes, which this receiver
/// overruns: it bursts around 1.5KB of sentences once a second. An overrun does not report
/// itself — it splices two sentences into one whose checksum then fails.
const RX_FIFO: usize = 4096;

/// How long to listen on a candidate pin before deciding the receiver is not on it. Sentences
/// arrive about once a second, so this leaves margin.
const PROBE: TickType = TickType::new_millis(3_000);

/// How long a read blocks once the pin is settled. Short enough that the loop keeps turning
/// while the receiver is quiet.
const READ: TickType = TickType::new_millis(200);

/// One read's worth. A burst is larger than this, and arrives over several reads; the UART's
/// own ring buffer is what has to hold a whole one.
const CHUNK: usize = 256;

/// How the receiver's UART wants configuring.
pub fn config() -> Config {
    Config::new().baudrate(BAUDRATE.Hz()).rx_fifo_size(RX_FIFO)
}

/// A UART the receiver might be transmitting into, and the sentences it produces.
pub struct Gnss<'d> {
    uart: UartRxDriver<'d>,
    /// The GPIO it reads. Logged, and the one thing about the wiring a console can confirm.
    pin: i32,
    /// Bytes read that do not yet end a line: a burst splits across reads.
    pending: String,
    bytes: [u8; CHUNK],
}

impl<'d> Gnss<'d> {
    pub fn new(uart: UartRxDriver<'d>, pin: i32) -> Self {
        Self {
            uart,
            pin,
            pending: String::new(),
            bytes: [0; CHUNK],
        }
    }

    /// Whichever candidate the receiver is transmitting into, or nothing if none is.
    ///
    /// **The Stick's RX is G33**; community sources say G32 and are wrong. Choosing wrong is
    /// indistinguishable from a dead receiver, so rather than trust either source, every
    /// candidate is opened and whichever carries NMEA wins. The pins are electrically
    /// independent, so listening on the idle one costs nothing.
    pub fn listening(candidates: impl IntoIterator<Item = Self>) -> Option<Self> {
        candidates.into_iter().find(|candidate| {
            let mut bytes = [0; CHUNK];
            let read = candidate.uart.read(&mut bytes, PROBE.ticks()).unwrap_or(0);
            // A `$` starts every sentence. Any bytes at all would be too weak a test: an idle
            // pin picks up noise, and the wrong pin winning is indistinguishable from a
            // receiver that never gets a fix.
            let sentences = bytes[..read].contains(&b'$');

            log::info!("probed G{}: {read} bytes, NMEA: {sentences}", candidate.pin);
            sentences
        })
    }

    pub fn pin(&self) -> i32 {
        self.pin
    }

    /// Whatever the receiver has sent since the last call, as whole sentences.
    ///
    /// Empty where nothing complete arrived, which is most calls: the loop turns far faster
    /// than the receiver talks. A line not shaped like a sentence is dropped, since a poor
    /// aerial produces those by the second and there is nothing to be done about one.
    pub fn sentences(&mut self) -> Vec<Sentence> {
        let read = self.uart.read(&mut self.bytes, READ.ticks()).unwrap_or(0);
        self.pending
            .push_str(&String::from_utf8_lossy(&self.bytes[..read]));

        let mut sentences = Vec::new();
        while let Some(end) = self.pending.find('\n') {
            let line: String = self.pending.drain(..=end).collect();
            match Sentence::new(line) {
                Ok(sentence) => sentences.push(sentence),
                Err(malformed) => log::debug!("dropped a line: {malformed}"),
            }
        }
        sentences
    }
}
