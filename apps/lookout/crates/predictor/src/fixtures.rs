//! Sentences in the shape the receiver emits them, for tests on either side of the parser.
//!
//! One place knows what an AT6668 sentence looks like: the field count, the NMEA 4.1
//! mode/status pair RMC ends with, the empty course a stationary receiver reports. A test
//! needing a fix somewhere else asks for one here rather than writing out another sentence,
//! so a change in the receiver is a change in one file.
//!
//! The constants are captured — real sentences the receiver emitted, with the position
//! replaced, since a real fix pins down where and when someone was. [`Fix`] builds the rest,
//! and the tests below hold it to those captures, so what it builds is what the receiver
//! sends.
//!
//! Off by default, behind the `fixtures` feature, so nothing here reaches a device binary. A
//! crate wanting it puts `predictor` in its `[dev-dependencies]` with the feature on.

use chrono::{DateTime, TimeZone, Utc};

use crate::sentence::Sentence;

/// Captured indoors, before the receiver had a fix. `RMC_VOID` carries the NMEA 4.1
/// navigational-status field — the trailing `,V` — that a hand-written 0183 RMC lacks.
pub const RMC_VOID: &str = "$GNRMC,202725.00,V,,,,,,,290726,,,N,V*11";
pub const GGA_NO_FIX: &str = "$GNGGA,202725.00,,,,,0,00,25.5,,,,,,*4A";
pub const GSA_NO_FIX: &str = "$GNGSA,A,1,,,,,,,,,,,,,25.5,25.5,25.5,1*01";
/// Also captured: an RX overrun spliced two sentences together. A shell sizes its UART ring
/// buffer to avoid the overrun, but corruption on a serial line is never ruled out, so
/// anything reading sentences has to survive one.
pub const SPLICED: &str = "$GAGSV,12724.00,V,N*55";

/// The date every built sentence carries, `ddmmyy` as RMC spells it. GGA has no date field,
/// which is why a stream reports nothing until its first RMC.
const DATE: &str = "290726";
/// The same day as [`DATE`], for a test asserting when a sample landed.
const YEAR: i32 = 2026;
const MONTH: u32 = 7;
const DAY: u32 = 29;

/// What the captured GGA reported about the fix it carried: quality, satellites, HDOP,
/// altitude in metres, and geoid separation. Fixed, because a test wanting different numbers
/// wants a different capture, not a different builder.
const QUALITY: &str = "1,06,4.4";
const ALTITUDE: &str = "262.46,M,45.12,M";

/// A captured sentence, as a [`Sentence`].
///
/// Infallible for the constants above: each is a real line off the receiver, and the shape is
/// all a [`Sentence`] asks for.
pub fn captured(sentence: &str) -> Sentence {
    Sentence::new(sentence).expect("a captured sentence")
}

/// Wraps a sentence body into the on-the-wire form: `$`, the body, then `*` and the XOR of
/// every body byte as two hex digits.
fn sentence(body: &str) -> Sentence {
    Sentence::new(format!("${body}*{:02X}", checksum(body))).expect("a body and its checksum")
}

/// The same sentence, corrupted: its contents intact and its checksum guaranteed wrong.
///
/// Still a [`Sentence`], because a checksum that fails to cover its body is the shape a real
/// overrun takes. Inverting every bit cannot land back on the correct value, which fabricating
/// one by hand can.
pub fn with_bad_checksum(sentence: &Sentence) -> Sentence {
    let body = sentence.body();

    Sentence::new(format!("${body}*{:02X}", checksum(body) ^ 0xFF)).expect("a wrong checksum")
}

fn checksum(body: &str) -> u8 {
    body.bytes().fold(0u8, |acc, byte| acc ^ byte)
}

/// One fix, as the sentences carrying it.
///
/// Build one with [`Fix::at`] and add what the receiver would have reported. A fix with no
/// speed set is a receiver standing still, which reports a speed of zero and no course at all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fix {
    hour: u32,
    minute: u32,
    second: u32,
    latitude_degrees: f64,
    longitude_degrees: f64,
    speed_knots: f64,
    course_degrees: Option<f64>,
}

impl Fix {
    /// A fix at a time of day, on the day every fixture is dated.
    pub fn at(
        hour: u32,
        minute: u32,
        second: u32,
        latitude_degrees: f64,
        longitude_degrees: f64,
    ) -> Self {
        Self {
            hour,
            minute,
            second,
            latitude_degrees,
            longitude_degrees,
            speed_knots: 0.0,
            course_degrees: None,
        }
    }

    /// Knots, which is what RMC reports and what a sample converts away from.
    pub fn with_speed_knots(self, speed_knots: f64) -> Self {
        Self {
            speed_knots,
            ..self
        }
    }

    pub fn with_course_degrees(self, course_degrees: f64) -> Self {
        Self {
            course_degrees: Some(course_degrees),
            ..self
        }
    }

    /// When this fix is, which is what a sample built from it carries.
    pub fn t(&self) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(YEAR, MONTH, DAY, self.hour, self.minute, self.second)
            .single()
            .expect("an instant on a real day")
    }

    /// Position, speed, course and date, ready for the wire.
    pub fn rmc(&self) -> Sentence {
        sentence(&self.rmc_body())
    }

    /// Position, fix quality and altitude, ready for the wire. No date: GGA carries none.
    pub fn gga(&self) -> Sentence {
        sentence(&self.gga_body())
    }

    fn rmc_body(&self) -> String {
        let course = match self.course_degrees {
            Some(degrees) => format!("{degrees:.2}"),
            None => String::new(),
        };
        format!(
            "GNRMC,{},A,{},{:.2},{course},{DATE},,,A,V",
            self.time(),
            self.position(),
            self.speed_knots,
        )
    }

    fn gga_body(&self) -> String {
        format!(
            "GNGGA,{},{},{QUALITY},{ALTITUDE},,",
            self.time(),
            self.position(),
        )
    }

    /// `hhmmss.ss`, to the hundredth of a second the receiver reports.
    fn time(&self) -> String {
        format!("{:02}{:02}{:02}.00", self.hour, self.minute, self.second)
    }

    /// Both axes with their hemispheres, in the degrees-and-decimal-minutes NMEA uses.
    fn position(&self) -> String {
        format!(
            "{},{},{},{}",
            degrees_and_minutes(self.latitude_degrees, 2),
            if self.latitude_degrees < 0.0 {
                "S"
            } else {
                "N"
            },
            degrees_and_minutes(self.longitude_degrees, 3),
            if self.longitude_degrees < 0.0 {
                "W"
            } else {
                "E"
            },
        )
    }
}

/// `ddmm.mmmmm`: whole degrees, then the remainder as minutes. `digits` is how wide the degrees
/// are — two for a latitude, three for a longitude — and the hemisphere is a separate field, so
/// what is formatted here is the magnitude.
fn degrees_and_minutes(degrees: f64, digits: usize) -> String {
    let degrees = degrees.abs();
    let whole = degrees.trunc();
    format!(
        "{:0digits$}{:08.5}",
        whole as u32,
        (degrees - whole) * 60.0,
        digits = digits,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bodies of sentences taken from a real outdoor capture, with the position replaced. They
    /// are what [`Fix`] is held to: everything else here builds sentences, and these are the
    /// evidence that what it builds is what the receiver emits.
    const CAPTURED_RMC: &str =
        "GNRMC,204329.00,A,5030.00000,N,00830.00000,E,4.13,79.94,290726,,,A,V";
    const CAPTURED_GGA: &str =
        "GNGGA,204329.00,5030.00000,N,00830.00000,E,1,06,4.4,262.46,M,45.12,M,,";
    /// A stationary receiver leaves the course field empty — the `0.08,,` here.
    const CAPTURED_RMC_STATIONARY: &str =
        "GNRMC,204858.00,A,5030.00000,N,00830.00000,E,0.08,,290726,,,A,V";

    fn captured_fix() -> Fix {
        Fix::at(20, 43, 29, 50.5, 8.5)
            .with_speed_knots(4.13)
            .with_course_degrees(79.94)
    }

    #[test]
    fn a_built_rmc_is_the_captured_one() {
        assert_eq!(captured_fix().rmc(), sentence(CAPTURED_RMC));
    }

    #[test]
    fn a_built_gga_is_the_captured_one() {
        assert_eq!(captured_fix().gga(), sentence(CAPTURED_GGA));
    }

    /// The course field is empty rather than zero, which is the shape that once broke a parser.
    #[test]
    fn a_fix_with_no_course_is_the_captured_stationary_one() {
        let stationary = Fix::at(20, 48, 58, 50.5, 8.5).with_speed_knots(0.08);

        assert_eq!(stationary.rmc(), sentence(CAPTURED_RMC_STATIONARY));
    }

    /// A minute is a sixtieth of a degree, and the field is degrees followed by minutes — so
    /// 51.0403 N is 51 degrees and 2.418 minutes, not 51.0403 of anything.
    #[test]
    fn a_coordinate_is_degrees_then_minutes() {
        assert_eq!(degrees_and_minutes(51.0403, 2), "5102.41800");
        assert_eq!(degrees_and_minutes(13.7322, 3), "01343.93200");
    }

    /// The hemisphere is its own field, so the magnitude is what is formatted.
    #[test]
    fn a_southern_or_western_fix_reports_its_hemisphere() {
        let fix = Fix::at(20, 43, 29, -33.9, -18.4);

        assert!(fix.rmc().body().contains("3354.00000,S,01824.00000,W"));
    }

    /// The contents have to survive, or a reader would refuse the sentence for the wrong
    /// reason — being unreadable rather than being unbelievable.
    #[test]
    fn a_corrupted_sentence_keeps_its_body_and_loses_its_checksum() {
        let corrupt = with_bad_checksum(&captured_fix().gga());

        assert_eq!(corrupt.body(), CAPTURED_GGA);
        assert_ne!(corrupt, captured_fix().gga());
    }

    #[test]
    fn a_fix_knows_when_it_is() {
        assert_eq!(
            captured_fix().t(),
            Utc.with_ymd_and_hms(2026, 7, 29, 20, 43, 29).unwrap()
        );
    }
}
