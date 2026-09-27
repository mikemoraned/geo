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
use m5_core::Device;
use mipidsi::{
    Builder,
    interface::SpiInterface,
    models::ST7789,
    options::{ColorInversion, Orientation, Rotation},
};
use platform_core::{Effect, Event, Lookout};

use m5plus::{battery, gnss, gnss::Gnss, panel, panel::Panel};

const DISPLAY_BUFFER: usize = 512;

const BATTERY_INTERVAL_S: u64 = 1;

const REPORT_INTERVAL_S: u64 = 60;

const REST_MS: u32 = 10;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;

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
    let display = Builder::new(ST7789, interface)
        .reset_pin(rst)
        .display_size(panel::WIDTH, panel::HEIGHT)
        .display_offset(panel::OFFSET_X, panel::OFFSET_Y)
        .invert_colors(ColorInversion::Inverted)
        .orientation(Orientation::new().rotate(Rotation::Deg0))
        .init(&mut Ets)
        .expect("the display initialises, or there is nothing to report a failure on");
    let mut panel: Panel<_> = Panel::new(display).expect("a cleared display");

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
        return Err("no NMEA on either Grove pin — check the unit is seated and powered".into());
    };
    log::info!("GNSS on G{} at {} baud", gnss.pin(), gnss::BAUDRATE);

    let adc = AdcDriver::new(peripherals.adc1)?;
    let mut battery_pin = AdcChannelDriver::new(&adc, peripherals.pins.gpio38, &battery::config())?;

    let core: Box<Core<Lookout<Device>>> = Box::new(Core::new());
    log::info!(
        "carrying {} crossings; {} bytes of main task stack never used",
        m5_core::carried::crossings().len(),
        stack_unused(),
    );

    let mut battery_read = Instant::now();
    let mut reported = Instant::now();

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
                Effect::Crossings(_) => {
                    core.process_event(Event::Crossings(Vec::new()));
                }
            }
        }

        FreeRtos::delay_ms(REST_MS);
    }
}

fn stack_unused() -> u32 {
    // Safe: a null task handle means the calling task, and this reads FreeRTOS's own
    // bookkeeping without touching it.
    unsafe { esp_idf_svc::sys::uxTaskGetStackHighWaterMark(std::ptr::null_mut()) }
}

fn free_heap() -> u32 {
    // Safe: it reads the allocator's own accounting.
    unsafe { esp_idf_svc::sys::esp_get_free_heap_size() }
}
