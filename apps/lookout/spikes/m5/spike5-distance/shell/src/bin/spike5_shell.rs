use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use crux_core::Core;
use embedded_graphics::{
    mono_font::{MonoTextStyleBuilder, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::*,
    text::Text,
};
use esp_idf_svc::hal::{
    delay::{Ets, FreeRtos, TickType},
    gpio::PinDriver,
    peripherals::Peripherals,
    spi::{SpiDeviceDriver, config::Config, config::DriverConfig},
    uart::{UartRxDriver, config::Config as UartConfig},
    units::FromValueType,
};
use mipidsi::{
    Builder,
    interface::SpiInterface,
    models::ST7789,
    options::{ColorInversion, Orientation, Rotation},
};
use spike5_core::{Effect, Event, Gnss, ViewModel};

/// Panel geometry and wiring, from M5GFX's `board_M5StickCPlus2`. See spike 1 for why the
/// offset matters and why the bus runs at 26MHz rather than M5GFX's nominal 40MHz.
const WIDTH: u16 = 135;
const HEIGHT: u16 = 240;
const OFFSET_X: u16 = 52;
const OFFSET_Y: u16 = 40;
const SPI_BAUDRATE: u32 = 26;

/// The GPS/BDS Unit v1.1 (AT6668) talks NMEA 0183 at 115200 8N1.
const GNSS_BAUDRATE: u32 = 115200;

/// How long to listen on each candidate Grove pin before deciding which one the receiver
/// is transmitting into. Sentences arrive about once a second, so this leaves margin.
const PIN_PROBE: TickType = TickType::new_millis(3000);

/// Blocking read budget once the RX pin is settled — short enough that the clock still
/// ticks about once a second when the receiver goes quiet.
const READ_TIMEOUT: TickType = TickType::new_millis(200);

/// Wall-clock time as the core wants it. Without NTP this counts from the epoch at boot;
/// the GNSS fix carries the true UTC, which is the point of this spike.
fn now() -> Option<DateTime<Utc>> {
    let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;

    DateTime::from_timestamp(
        since_epoch.as_secs().try_into().ok()?,
        since_epoch.subsec_nanos(),
    )
}

/// Which Grove pin the receiver is actually transmitting into.
///
/// M5's own examples disagree about whether the Stick's RX is G32 or G33, and the wrong
/// choice looks identical to a dead receiver. Rather than guess, both are opened at once
/// on separate UARTs and whichever carries NMEA wins — the two are electrically
/// independent, so listening on the idle one costs nothing.
fn listening_pin<'d>(
    candidates: [(UartRxDriver<'d>, i32); 2],
    buffer: &mut [u8],
) -> Option<(UartRxDriver<'d>, i32)> {
    candidates.into_iter().find(|(uart, pin)| {
        let read = uart.read(buffer, PIN_PROBE.ticks()).unwrap_or(0);
        let sentences = buffer[..read].contains(&b'$');

        log::info!("probed G{pin}: {read} bytes, NMEA: {sentences}");
        sentences
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;

    // Established by spike 0: G4 high keeps the device powered once off USB, and the
    // driver has to outlive the program for the pin to stay asserted.
    let mut hold = PinDriver::output(peripherals.pins.gpio4)?;
    hold.set_high()?;

    let spi = SpiDeviceDriver::new_single(
        peripherals.spi2,
        peripherals.pins.gpio13,
        peripherals.pins.gpio15,
        Option::<esp_idf_svc::hal::gpio::AnyIOPin>::None,
        Some(peripherals.pins.gpio5),
        &DriverConfig::new(),
        &Config::new().baudrate(SPI_BAUDRATE.MHz().into()),
    )?;

    let dc = PinDriver::output(peripherals.pins.gpio14)?;
    let rst = PinDriver::output(peripherals.pins.gpio12)?;

    let mut buffer = [0u8; 512];
    let di = SpiInterface::new(spi, dc, &mut buffer);

    // Panicking here is the intended behaviour: a display that won't initialise leaves
    // this spike with nothing to show, and there is no recovery worth attempting.
    let mut display = Builder::new(ST7789, di)
        .reset_pin(rst)
        .display_size(WIDTH, HEIGHT)
        .display_offset(OFFSET_X, OFFSET_Y)
        .invert_colors(ColorInversion::Inverted)
        .orientation(Orientation::new().rotate(Rotation::Deg0))
        .init(&mut Ets)
        .expect("display initialisation");

    display.clear(Rgb565::BLACK).expect("clear display");

    let mut backlight = PinDriver::output(peripherals.pins.gpio27)?;
    backlight.set_high()?;

    let style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::CSS_ORANGE)
        .background_color(Rgb565::BLACK)
        .build();

    // The default RX ring buffer is `UART_FIFO_SIZE * 2` — 256 bytes, which this receiver
    // overruns: it emits a burst of a dozen-plus sentences (~1.5KB) once a second, and
    // anything the shell does meanwhile is long enough to lose bytes. Overruns don't report
    // themselves, they just splice two sentences together and fail the checksum.
    let uart_config = UartConfig::new()
        .baudrate(GNSS_BAUDRATE.Hz())
        .rx_fifo_size(4096);
    let on_gpio32 = UartRxDriver::new(
        peripherals.uart1,
        peripherals.pins.gpio32,
        Option::<esp_idf_svc::hal::gpio::AnyIOPin>::None,
        Option::<esp_idf_svc::hal::gpio::AnyIOPin>::None,
        &uart_config,
    )?;
    let on_gpio33 = UartRxDriver::new(
        peripherals.uart2,
        peripherals.pins.gpio33,
        Option::<esp_idf_svc::hal::gpio::AnyIOPin>::None,
        Option::<esp_idf_svc::hal::gpio::AnyIOPin>::None,
        &uart_config,
    )?;

    let mut bytes = [0u8; 256];
    let Some((gnss, pin)) = listening_pin([(on_gpio32, 32), (on_gpio33, 33)], &mut bytes) else {
        // Nothing to render and nothing to retry: either the unit is unplugged or the baud
        // rate is wrong, both of which need a human.
        return Err("no NMEA on either Grove pin — check the unit is seated and powered".into());
    };
    log::info!("GNSS on G{pin} at {GNSS_BAUDRATE} baud");

    // Boxed so the model lives on the heap. It embeds the `nmea` accumulator, which carries
    // per-constellation satellite tables and is several KB on its own, and it only grows as
    // the core takes on more state — none of which belongs in a task's stack frame.
    let core: Box<Core<Gnss>> = Box::new(Core::new());
    // Safe: a null task handle means "the calling task", and this only reads FreeRTOS's own
    // bookkeeping. Logged because an overflow here corrupts memory silently rather than
    // reporting itself — it showed up as a bad pointer deep inside an unrelated driver.
    let stack_unused = unsafe { esp_idf_svc::sys::uxTaskGetStackHighWaterMark(std::ptr::null_mut()) };
    log::info!("core up; {stack_unused} bytes of main task stack never used");

    let mut ticked = Instant::now();
    // NMEA sentences arrive split across reads, so bytes accumulate here until a newline.
    let mut pending = String::new();
    // What the panel currently shows, so an unchanged view model costs no SPI traffic.
    let mut shown: Option<ViewModel> = None;

    loop {
        let read = gnss.read(&mut bytes, READ_TIMEOUT.ticks()).unwrap_or(0);
        pending.push_str(&String::from_utf8_lossy(&bytes[..read]));

        let mut effects = Vec::new();
        while let Some(end) = pending.find('\n') {
            let sentence: String = pending.drain(..=end).collect();
            let sentence = sentence.trim().to_string();

            // Echoed verbatim so a monitor session doubles as a capture: the core's tests
            // currently run on hand-written sentences, and this is where real ones come
            // from. `just capture` saves the console to a file.
            log::info!("{sentence}");

            effects.extend(core.process_event(Event::Sentence(sentence)));
        }

        if ticked.elapsed().as_secs() >= 1 {
            ticked = Instant::now();
            if let Some(now) = now() {
                effects.extend(core.process_event(Event::Tick(now)));
            }
        }

        // The core decides what the screen should say; the shell only carries out the
        // effects it asks for.
        for effect in effects {
            match effect {
                Effect::Render(_) => {
                    // A render is asked for per sentence — a dozen-plus a second, most of
                    // which change nothing on screen. Redrawing each one holds the SPI bus
                    // long enough to lose incoming NMEA, so unchanged views are skipped.
                    let view = core.view();
                    if shown.as_ref() != Some(&view) {
                        for (line, y) in
                            [(&view.clock, 30), (&view.latitude, 60), (&view.longitude, 85)]
                        {
                            Text::new(line, Point::new(8, y), style)
                                .draw(&mut display)
                                .expect("draw view model");
                        }
                        shown = Some(view);
                    }
                }
            }
        }

        FreeRtos::delay_ms(10);
    }
}
