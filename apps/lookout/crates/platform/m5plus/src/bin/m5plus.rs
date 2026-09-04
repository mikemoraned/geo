//! Boots the board and runs its loop: sentences and voltages into the core, a panel out.
//!
//! Every board fact here is established by running code on this hardware, and several
//! contradict the vendor and community documentation. See `apps/lookout/docs/device.md`.

use std::time::Instant;

use crux_core::Core;
use esp_idf_svc::hal::{
    adc::oneshot::{AdcChannelDriver, AdcDriver},
    delay::{Ets, FreeRtos},
    gpio::{AnyIOPin, PinDriver},
    peripherals::Peripherals,
    spi::{SpiDeviceDriver, config::Config, config::DriverConfig},
    uart::UartRxDriver,
    units::FromValueType,
};
use mipidsi::{
    Builder,
    interface::SpiInterface,
    models::ST7789,
    options::{ColorInversion, Orientation, Rotation},
};
use platform_core::{Effect, Event, Lookout};

use m5plus::{battery, gnss, gnss::Gnss, panel, panel::Panel};

/// The display interface's own scratch buffer, sized for a line of pixels rather than a frame:
/// nothing here draws more than a row of text at a time.
const DISPLAY_BUFFER: usize = 512;

/// How often the battery is read. A cell changes over hours, but a conversion is cheap and it
/// keeps the shell's job to reading the pin.
const BATTERY_INTERVAL_S: u64 = 1;

/// How often the console gets a line about the state of the board. Often enough to watch a
/// discharge, rare enough not to crowd the log.
const REPORT_INTERVAL_S: u64 = 60;

/// How long the loop rests once it has read the receiver and drawn what changed.
const REST_MS: u32 = 10;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;

    // **G4 high is what keeps the board alive on battery**, and the driver has to outlive the
    // program: dropping it resets the pin and cuts power. On USB the board runs either way,
    // which is how this is missed. Set before anything else, so a slow start on battery is
    // still a start.
    let mut hold = PinDriver::output(peripherals.pins.gpio4)?;
    hold.set_high()?;

    let spi = SpiDeviceDriver::new_single(
        peripherals.spi2,
        peripherals.pins.gpio13,
        peripherals.pins.gpio15,
        Option::<AnyIOPin>::None,
        Some(peripherals.pins.gpio5),
        &DriverConfig::new(),
        &Config::new().baudrate(panel::SPI_MEGAHERTZ.MHz().into()),
    )?;
    let dc = PinDriver::output(peripherals.pins.gpio14)?;
    let rst = PinDriver::output(peripherals.pins.gpio12)?;

    let mut interface_buffer = [0u8; DISPLAY_BUFFER];
    let interface = SpiInterface::new(spi, dc, &mut interface_buffer);
    // A display that will not initialise leaves the device with no output at all, and there is
    // no recovery worth attempting.
    let display = Builder::new(ST7789, interface)
        .reset_pin(rst)
        .display_size(panel::WIDTH, panel::HEIGHT)
        .display_offset(panel::OFFSET_X, panel::OFFSET_Y)
        .invert_colors(ColorInversion::Inverted)
        .orientation(Orientation::new().rotate(Rotation::Deg0))
        .init(&mut Ets)
        .expect("the display initialises, or there is nothing to report a failure on");
    let mut panel: Panel<_> = Panel::new(display).expect("a cleared display");

    // Only now: until the panel has been drawn once, the backlight would show whatever the
    // controller powered up with.
    let mut backlight = PinDriver::output(peripherals.pins.gpio27)?;
    backlight.set_high()?;

    let config = gnss::config();
    let candidates = [
        (
            UartRxDriver::new(
                peripherals.uart2,
                peripherals.pins.gpio33,
                Option::<AnyIOPin>::None,
                Option::<AnyIOPin>::None,
                &config,
            )?,
            33,
        ),
        (
            UartRxDriver::new(
                peripherals.uart1,
                peripherals.pins.gpio32,
                Option::<AnyIOPin>::None,
                Option::<AnyIOPin>::None,
                &config,
            )?,
            32,
        ),
    ];
    let Some(mut gnss) = Gnss::listening(
        candidates
            .into_iter()
            .map(|(uart, pin)| Gnss::new(uart, pin)),
    ) else {
        // Nothing to render and nothing to retry: either the unit is unseated or the baud rate
        // is wrong, and both need a human.
        return Err("no NMEA on either Grove pin — check the unit is seated and powered".into());
    };
    log::info!("GNSS on G{} at {} baud", gnss.pin(), gnss::BAUDRATE);

    // **Battery voltage is GPIO38 on ADC1.** The PLUS2 has no PMIC to ask.
    let adc = AdcDriver::new(peripherals.adc1)?;
    let mut battery_pin = AdcChannelDriver::new(&adc, peripherals.pins.gpio38, &battery::config())?;

    // Boxed so the model lives on the heap rather than in this task's frame. It embeds a parser
    // carrying per-constellation satellite tables, several KB on its own, and it only grows.
    let core: Box<Core<Lookout>> = Box::new(Core::new());
    log::info!(
        "carrying {} crossings; {} bytes of main task stack never used",
        platform_core::carried::crossings().len(),
        stack_unused(),
    );

    let mut battery_read = Instant::now();
    let mut reported = Instant::now();

    // No `Event::Tick` is sent: this board's clock counts from the epoch at boot, with no NTP
    // and no RTC to set it from, so every tick would be behind the receiver and refused. The
    // panel's clock and its countdowns run on the receiver's time, which arrives with each fix.
    loop {
        let mut effects = Vec::new();

        for sentence in gnss.sentences() {
            effects.extend(core.process_event(Event::Sentence(sentence)));
        }

        if battery_read.elapsed().as_secs() >= BATTERY_INTERVAL_S {
            battery_read = Instant::now();
            match battery_pin.read() {
                Ok(at_pin) => {
                    let millivolts = battery::terminal_millivolts(at_pin);
                    if reported.elapsed().as_secs() >= REPORT_INTERVAL_S {
                        reported = Instant::now();
                        // The voltage raw as well as bars: the bars are deliberately too coarse
                        // to check a divider or a calibration against. The stack and the heap
                        // because a number over a run is what distinguishes a leak from a
                        // level, and neither reports itself before it is fatal.
                        log::info!(
                            "battery {millivolts}mV ({at_pin}mV at the pin); \
                             {} bytes of main task stack never used, {} bytes of free heap",
                            stack_unused(),
                            free_heap(),
                        );
                    }
                    effects.extend(core.process_event(Event::Battery(millivolts)));
                }
                Err(unread) => log::warn!("battery read failed: {unread}"),
            }
        }

        for effect in effects {
            match effect {
                Effect::Render(_) => panel.show(core.view()).expect("draw the panel"),
            }
        }

        FreeRtos::delay_ms(REST_MS);
    }
}

/// How much of the main task's stack has never been touched.
///
/// **Stack overflow has been the cause of every hard-to-diagnose crash on this board, and it
/// never presents as one** — it lands as a fault in whatever code is nearby. Logging a number
/// at startup means there is one to compare against rather than an estimate.
fn stack_unused() -> u32 {
    // Safe: a null task handle means the calling task, and this reads FreeRTOS's own
    // bookkeeping without touching it.
    unsafe { esp_idf_svc::sys::uxTaskGetStackHighWaterMark(std::ptr::null_mut()) }
}

/// How much heap is free.
///
/// Nothing copies the crossings out of flash, so this sits flat however many scans have run.
/// A number that walks down over a run says otherwise.
fn free_heap() -> u32 {
    // Safe: it reads the allocator's own accounting.
    unsafe { esp_idf_svc::sys::esp_get_free_heap_size() }
}
