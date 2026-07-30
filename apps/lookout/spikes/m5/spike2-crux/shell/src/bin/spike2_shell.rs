use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use embedded_graphics::{
    mono_font::{MonoTextStyleBuilder, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::*,
    text::Text,
};
use esp_idf_svc::hal::{
    delay::{Ets, FreeRtos},
    gpio::PinDriver,
    peripherals::Peripherals,
    spi::{SpiDeviceDriver, config::Config, config::DriverConfig},
    units::FromValueType,
};
use mipidsi::{
    Builder,
    interface::SpiInterface,
    models::ST7789,
    options::{ColorInversion, Orientation, Rotation},
};
use spike2_core::{Clock, Effect, Event};

use crux_core::Core;

/// Panel geometry and wiring, from M5GFX's `board_M5StickCPlus2`. See spike 1 for why the
/// offset matters and why the bus runs at 26MHz rather than M5GFX's nominal 40MHz.
const WIDTH: u16 = 135;
const HEIGHT: u16 = 240;
const OFFSET_X: u16 = 52;
const OFFSET_Y: u16 = 40;
const SPI_BAUDRATE: u32 = 26;

/// Wall-clock time as the core wants it. Without NTP or an RTC read this counts from the
/// epoch at boot, so it ticks correctly but is not the real date — spike 3's GNSS fix is
/// where a true time arrives.
fn now() -> Option<DateTime<Utc>> {
    let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;

    DateTime::from_timestamp(
        since_epoch.as_secs().try_into().ok()?,
        since_epoch.subsec_nanos(),
    )
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

    let core: Core<Clock> = Core::new();
    log::info!("core up; ticking once a second");

    loop {
        if let Some(now) = now() {
            // The core decides what the screen should say; the shell only carries out the
            // effects it asks for.
            for effect in core.process_event(Event::Tick(now)) {
                match effect {
                    Effect::Render(_) => {
                        Text::new(&core.view().clock, Point::new(8, 40), style)
                            .draw(&mut display)
                            .expect("draw clock");
                    }
                }
            }
        }

        FreeRtos::delay_ms(1000);
    }
}
