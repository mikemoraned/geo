use embedded_graphics::{
    mono_font::{MonoTextStyle, MonoTextStyleBuilder, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::*,
    text::Text,
};
use m5_core::{NEAREST_ON_SCREEN, ViewModel};

pub const WIDTH: u16 = 135;
pub const HEIGHT: u16 = 240;
pub const OFFSET_X: u16 = 52;
pub const OFFSET_Y: u16 = 40;

pub const SPI_MEGAHERTZ: u32 = 26;

const CHARACTERS_PER_LINE: usize = 13;
const CHARACTER_WIDTH: i32 = 10;
const CLOCK_CHARACTERS: usize = 8;
const LINE_HEIGHT: i32 = 22;
const MARGIN_X: i32 = 4;
const FIRST_LINE_Y: i32 = 20;
const CROSSINGS_Y: i32 = 130;

pub struct Panel<D> {
    display: D,
    style: MonoTextStyle<'static, Rgb565>,
    shown: Option<ViewModel>,
}

impl<D: DrawTarget<Color = Rgb565>> Panel<D> {
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

    pub fn show(&mut self, view: ViewModel) -> Result<(), D::Error> {
        if self.shown.as_ref() == Some(&view) {
            return Ok(());
        }

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
