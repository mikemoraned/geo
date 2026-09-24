use chrono::{DateTime, TimeZone, Utc};

use crate::sentence::Sentence;

// Captured indoors, before the receiver had a fix. RMC_VOID ends with the NMEA 4.1
// navigational-status field — the trailing `,V` — that a hand-written 0183 RMC lacks.
pub const RMC_VOID: &str = "$GNRMC,202725.00,V,,,,,,,290726,,,N,V*11";
pub const GGA_NO_FIX: &str = "$GNGGA,202725.00,,,,,0,00,25.5,,,,,,*4A";
pub const GSA_NO_FIX: &str = "$GNGSA,A,1,,,,,,,,,,,,,25.5,25.5,25.5,1*01";
// Also captured: an RX overrun spliced two sentences into one, leaving a checksum that
// belongs to neither half.
pub const SPLICED: &str = "$GAGSV,12724.00,V,N*55";

// `ddmmyy`, as RMC spells a date.
const DATE: &str = "290726";
const YEAR: i32 = 2026;
const MONTH: u32 = 7;
const DAY: u32 = 29;

// What the captured GGA reported, in the order the sentence carries it.
const QUALITY_SATELLITES_HDOP: &str = "1,06,4.4";
const ALTITUDE_AND_GEOID_SEPARATION: &str = "262.46,M,45.12,M";

pub fn captured(sentence: &str) -> Sentence {
    Sentence::new(sentence).expect("a captured sentence")
}

fn sentence(body: &str) -> Sentence {
    Sentence::new(format!("${body}*{:02X}", checksum(body))).expect("a body and its checksum")
}

pub fn with_bad_checksum(sentence: &Sentence) -> Sentence {
    let body = sentence.body();

    Sentence::new(format!("${body}*{:02X}", checksum(body) ^ 0xFF)).expect("a wrong checksum")
}

fn checksum(body: &str) -> u8 {
    body.bytes().fold(0u8, |acc, byte| acc ^ byte)
}

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

    pub fn t(&self) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(YEAR, MONTH, DAY, self.hour, self.minute, self.second)
            .single()
            .expect("an instant on a real day")
    }

    pub fn rmc(&self) -> Sentence {
        sentence(&self.rmc_body())
    }

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
            "GNGGA,{},{},{QUALITY_SATELLITES_HDOP},{ALTITUDE_AND_GEOID_SEPARATION},,",
            self.time(),
            self.position(),
        )
    }

    fn time(&self) -> String {
        format!("{:02}{:02}{:02}.00", self.hour, self.minute, self.second)
    }

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

    // Captured bodies, checksum excluded, at 50.5N 8.5E: moving at 4.13 knots on a course of
    // 79.94°, and — in the third — stationary at 0.08 knots, which the receiver reports with the
    // course field left empty.
    const CAPTURED_RMC: &str =
        "GNRMC,204329.00,A,5030.00000,N,00830.00000,E,4.13,79.94,290726,,,A,V";
    const CAPTURED_GGA: &str =
        "GNGGA,204329.00,5030.00000,N,00830.00000,E,1,06,4.4,262.46,M,45.12,M,,";
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

    #[test]
    fn a_fix_with_no_course_is_the_captured_stationary_one() {
        let stationary = Fix::at(20, 48, 58, 50.5, 8.5).with_speed_knots(0.08);

        assert_eq!(stationary.rmc(), sentence(CAPTURED_RMC_STATIONARY));
    }

    #[test]
    fn a_coordinate_is_degrees_then_minutes() {
        assert_eq!(degrees_and_minutes(51.0403, 2), "5102.41800");
        assert_eq!(degrees_and_minutes(13.7322, 3), "01343.93200");
    }

    #[test]
    fn a_southern_or_western_fix_reports_its_hemisphere() {
        let fix = Fix::at(20, 43, 29, -33.9, -18.4);

        assert!(fix.rmc().body().contains("3354.00000,S,01824.00000,W"));
    }

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
