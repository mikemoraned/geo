//! The screen, and where each thing the core reports goes on it.
//!
//! The core has already formatted every line to fit; this places them. Generic over what it
//! draws into, so the ST7789's own type stays in `main` with the rest of the wiring.

use embedded_graphics::{
    mono_font::{MonoTextStyle, MonoTextStyleBuilder, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::*,
    text::Text,
};
use platform_core::{NEAREST_ON_SCREEN, ViewModel};

/// Panel geometry and wiring, from M5GFX's `board_M5StickCPlus2`. **The offset is the part
/// that has to be right**: the controller addresses a window larger than the visible panel,
/// so without it the image shifts and wraps.
pub const WIDTH: u16 = 135;
pub const HEIGHT: u16 = 240;
pub const OFFSET_X: u16 = 52;
pub const OFFSET_Y: u16 = 40;

/// The bus tops out here, not at the 40MHz M5GFX quotes: the panel is not on SPI2's IOMUX
/// pins, so its signals route through the GPIO matrix, which ESP-IDF caps at 80MHz/3. Asking
/// for more is rejected at `spi_bus_add_device`, and the device then boot-loops on a secondary
/// assert while tearing down a half-added device.
pub const SPI_MEGAHERTZ: u32 = 26;

/// `FONT_10X20` is ten pixels wide, so a 135-pixel panel holds thirteen characters. Every line
/// is drawn padded to its width: the display has no concept of erasing, so a shorter line
/// would leave the tail of the one before it on screen.
const CHARACTERS_PER_LINE: usize = 13;
const CHARACTER_WIDTH: i32 = 10;
/// `HH:MM:SS`. The five characters left on that line are the battery's.
const CLOCK_CHARACTERS: usize = 8;
const LINE_HEIGHT: i32 = 22;
const MARGIN_X: i32 = 4;
/// Baselines, not tops. The fix and its quality take five lines from here, and the crossings
/// start below a gap wide enough to read as a separate block; the last of them lands at 218,
/// clear of a 240-pixel panel.
const FIRST_LINE_Y: i32 = 20;
const CROSSINGS_Y: i32 = 130;

/// The screen, and what it is currently showing.
pub struct Panel<D> {
    display: D,
    style: MonoTextStyle<'static, Rgb565>,
    /// What was last drawn. A view model that has not moved costs no SPI traffic — and holding
    /// the bus to redraw an unchanged screen is long enough to lose incoming NMEA.
    shown: Option<ViewModel>,
}

impl<D: DrawTarget<Color = Rgb565>> Panel<D> {
    /// Clears the display and takes it over. Turn the backlight on after this, so the panel
    /// never briefly shows whatever the controller powered up with.
    pub fn new(mut display: D) -> Result<Self, D::Error> {
        display.clear(Rgb565::BLACK)?;

        Ok(Self {
            display,
            style: MonoTextStyleBuilder::new()
                .font(&FONT_10X20)
                .text_color(Rgb565::CSS_ORANGE)
                .background_color(Rgb565::BLACK)
                .build(),
            shown: None,
        })
    }

    /// Draws what the core says, where nothing has changed since the last draw.
    pub fn show(&mut self, view: ViewModel) -> Result<(), D::Error> {
        if self.shown.as_ref() == Some(&view) {
            return Ok(());
        }

        // The first line is shared: the clock takes eight characters, the battery the five
        // left of thirteen.
        self.write(
            &format!("{:width$}", view.clock, width = CLOCK_CHARACTERS),
            MARGIN_X,
            FIRST_LINE_Y,
        )?;
        self.write(
            &view.battery,
            MARGIN_X + CLOCK_CHARACTERS as i32 * CHARACTER_WIDTH,
            FIRST_LINE_Y,
        )?;

        let fix = [
            (view.latitude.as_str(), FIRST_LINE_Y + LINE_HEIGHT),
            (view.longitude.as_str(), FIRST_LINE_Y + 2 * LINE_HEIGHT),
            (view.quality.as_str(), FIRST_LINE_Y + 3 * LINE_HEIGHT),
            (view.within.as_str(), FIRST_LINE_Y + 4 * LINE_HEIGHT),
        ];
        // The list shortens as well as changes — from five crossings to none when a fix is
        // lost — so every line it can occupy is written every time, blank where there is
        // nothing, or the last one stays on screen.
        let nearest = (0..NEAREST_ON_SCREEN).map(|index| {
            (
                view.nearest.get(index).map_or("", String::as_str),
                CROSSINGS_Y + index as i32 * LINE_HEIGHT,
            )
        });

        for (line, y) in fix.into_iter().chain(nearest) {
            self.write(
                &format!("{line:width$}", width = CHARACTERS_PER_LINE),
                MARGIN_X,
                y,
            )?;
        }
        self.shown = Some(view);

        Ok(())
    }

    fn write(&mut self, line: &str, x: i32, y: i32) -> Result<(), D::Error> {
        Text::new(line, Point::new(x, y), self.style)
            .draw(&mut self.display)
            .map(|_| ())
    }
}
