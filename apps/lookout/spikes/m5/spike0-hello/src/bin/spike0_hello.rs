use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::PinDriver;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::sys::EspError;

/// Total PSRAM the ESP-IDF heap found at startup, in bytes. Zero means the probe failed —
/// `CONFIG_SPIRAM_IGNORE_NOTFOUND` turns that into a reportable result rather than an
/// aborted boot.
fn spiram_bytes() -> usize {
    // Safe: `heap_caps_get_total_size` only reads heap bookkeeping the IDF startup code
    // has already initialised by the time `main` runs.
    unsafe { esp_idf_svc::sys::heap_caps_get_total_size(esp_idf_svc::sys::MALLOC_CAP_SPIRAM) }
}

fn main() -> Result<(), EspError> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;

    // The PLUS2 latches its own power supply through G4. USB feeds the rail regardless,
    // but on battery the device switches off within milliseconds of boot unless G4 is
    // driven high — so it is set first, and `hold` is kept alive for the lifetime of the
    // program, since dropping the driver returns the pin to its default state.
    let mut hold = PinDriver::output(peripherals.pins.gpio4)?;
    hold.set_high()?;

    let mut led = PinDriver::output(peripherals.pins.gpio19)?;

    log::info!("hello from spike0 on an M5StickC PLUS2");
    log::info!("psram: {} bytes", spiram_bytes());

    let mut tick: u32 = 0;
    loop {
        // The red LED is active-low, and is the only liveness signal once USB — and with
        // it the serial console — is unplugged.
        led.set_low()?;
        FreeRtos::delay_ms(100);
        led.set_high()?;
        FreeRtos::delay_ms(900);

        tick += 1;
        log::info!("tick {tick}");
    }
}
