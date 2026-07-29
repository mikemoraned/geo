use embedded_graphics::{
    mono_font::{MonoTextStyle, MonoTextStyleBuilder, ascii::FONT_10X20},
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

/// Pixel dimensions of the visible panel, and where it sits inside the ST7789V2's larger
/// address window. Both come from M5GFX's own board definition for the PLUS2 — the
/// controller addresses a 240x320 window, so without the offset the image wraps.
const WIDTH: u16 = 135;
const HEIGHT: u16 = 240;
const OFFSET_X: u16 = 52;
const OFFSET_Y: u16 = 40;

/// The panel is not wired to SPI2's IOMUX pins (those are CLK 14 / MOSI 13 / CS 15), so
/// its signals route through the GPIO matrix, which ESP-IDF caps at 80MHz/3 ≈ 26.67MHz.
/// Asking for more is rejected outright at `spi_bus_add_device`, so M5GFX's nominal 40MHz
/// is not reachable through this driver.
const SPI_BAUDRATE: u32 = 26;

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

    // Batches pixel writes; larger is faster with diminishing returns.
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

    // The backlight is driven high only once there is something to show, so the panel
    // doesn't flash whatever the controller powered up with.
    let mut backlight = PinDriver::output(peripherals.pins.gpio27)?;
    backlight.set_high()?;

    display.clear(Rgb565::BLACK).expect("clear display");

    Text::new(
        "hello",
        Point::new(8, 30),
        MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE),
    )
    .draw(&mut display)
    .expect("draw greeting");

    log::info!("display up: {WIDTH}x{HEIGHT} at offset {OFFSET_X},{OFFSET_Y}");

    // A drawn-on background repaints each frame's glyphs, so the counter can be redrawn
    // in place without clearing the whole panel.
    let counter_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(Rgb565::CSS_ORANGE)
        .background_color(Rgb565::BLACK)
        .build();

    let mut tick: u32 = 0;
    loop {
        FreeRtos::delay_ms(1000);
        tick += 1;

        Text::new(
            &format!("tick {tick:>4}"),
            Point::new(8, 60),
            counter_style,
        )
        .draw(&mut display)
        .expect("draw tick");
    }
}
