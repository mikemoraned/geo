use esp_idf_svc::hal::{
    delay::TickType,
    uart::{UartRxDriver, config::Config},
    units::FromValueType,
};
use predictor::Sentence;

pub const BAUDRATE: u32 = 115_200;

const RX_FIFO: usize = 4096;

const PROBE: TickType = TickType::new_millis(3_000);

const READ: TickType = TickType::new_millis(200);

const CHUNK: usize = 256;

const SENTENCE_START: u8 = b'$';

pub fn config() -> Config {
    Config::new().baudrate(BAUDRATE.Hz()).rx_fifo_size(RX_FIFO)
}

pub struct Gnss<'d> {
    uart: UartRxDriver<'d>,
    pin: i32,
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

    pub fn listening(candidates: impl IntoIterator<Item = Self>) -> Option<Self> {
        candidates.into_iter().find(|candidate| {
            let mut bytes = [0; CHUNK];
            let read = candidate.uart.read(&mut bytes, PROBE.ticks()).unwrap_or(0);
            let sentences = bytes[..read].contains(&SENTENCE_START);

            log::info!("probed G{}: {read} bytes, NMEA: {sentences}", candidate.pin);
            sentences
        })
    }

    pub fn pin(&self) -> i32 {
        self.pin
    }

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
