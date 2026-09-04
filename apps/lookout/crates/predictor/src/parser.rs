//! NMEA sentences in, [`Sample`]s out.
//!
//! Only the device needs this. The simulation reads samples the store already holds, so it
//! never builds one from a sentence.

use nmea::Nmea;

use crate::measure::Measure;
use crate::sample::Sample;

/// One knot in metres per second, by definition — a nautical mile an hour, and a nautical
/// mile is 1,852 metres.
const METRES_PER_SECOND_PER_KNOT: f64 = 1_852.0 / 3_600.0;

/// Accumulates sentences into samples.
///
/// One fix is spread over several sentences: RMC carries the date, the speed and the course,
/// GGA the altitude, the satellite count and the HDOP. So the parser keeps state across them
/// and reports a sample from everything it knows, each time a sentence adds to it.
#[derive(Debug, Clone)]
pub struct Parser<T: Measure> {
    /// The `nmea` crate's own accumulator, which merges each sentence into the picture so
    /// far. A sentence carrying no position clears the position, so a sample is built from
    /// what the accumulator holds rather than from the sentence last parsed.
    sentences: Nmea,
    /// The last sample reported, so that a sentence adding nothing reports nothing.
    last: Option<Sample<T>>,
}

/// Hand-written, because deriving it would demand a `Default` measure that a parser with no
/// sample yet has no use for.
impl<T: Measure> Default for Parser<T> {
    fn default() -> Self {
        Self {
            sentences: Nmea::default(),
            last: None,
        }
    }
}

impl<T: Measure> Parser<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one sentence, exactly as it came off the wire, and reports the sample it
    /// completes.
    ///
    /// Nothing is reported until the receiver has a position and a date to place it on. GGA
    /// carries no date, so a stream reports its first sample when its first RMC lands.
    /// Three kinds of sentence report nothing and leave what is known intact: one that fails
    /// its checksum, one the receiver emits before it has a fix, and one that repeats what is
    /// already known.
    pub fn absorb(&mut self, sentence: &str) -> Option<Sample<T>> {
        self.sentences.parse(sentence).ok()?;

        let sample = self.sample()?;
        if self.last == Some(sample) {
            return None;
        }
        self.last = Some(sample);
        Some(sample)
    }

    /// Everything accumulated so far as a sample, once it amounts to one.
    fn sample(&self) -> Option<Sample<T>> {
        let at = self.sentences.fix_date?.and_time(self.sentences.fix_time?);

        Some(
            Sample::at(
                at.and_utc(),
                self.sentences.latitude?,
                self.sentences.longitude?,
            )
            .ok()?
            .with_altitude_metres(self.sentences.altitude.map(f64::from))
            .with_speed_mps(
                self.sentences
                    .speed_over_ground
                    .map(|knots| f64::from(knots) * METRES_PER_SECOND_PER_KNOT),
            )
            .with_heading_degrees(self.sentences.true_course.map(f64::from))
            .with_satellites(self.sentences.num_of_fix_satellites)
            .with_hdop(self.sentences.hdop.map(f64::from)),
        )
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::fixtures::{Fix, GGA_NO_FIX, GSA_NO_FIX, RMC_VOID, SPLICED, with_bad_checksum};

    /// The fix the captured sentences carry, at 50.5N 8.5E and moving.
    fn fix() -> Fix {
        Fix::at(20, 43, 29, 50.5, 8.5)
            .with_speed_knots(4.13)
            .with_course_degrees(79.94)
    }

    /// A second later and a little north-east of it.
    fn later() -> Fix {
        Fix::at(20, 43, 30, 50.5001, 8.50015)
            .with_speed_knots(4.13)
            .with_course_degrees(79.94)
    }

    /// The `nmea` crate holds everything but the coordinates as `f32`, so a field widened to
    /// `f64` lands near the decimal the sentence spells rather than on it.
    fn assert_near(got: Option<f64>, want: f64) {
        let got = got.expect("a value");
        assert!((got - want).abs() < 1e-5, "{got} is not near {want}");
    }

    /// The measure a test parser holds: `f64`, since nothing here is measuring distances.
    fn parser() -> Parser<f64> {
        Parser::new()
    }

    /// A parser that has seen the RMC every fix needs, since only RMC carries the date.
    fn fixed() -> Parser<f64> {
        let mut parser = parser();
        parser.absorb(&fix().rmc()).expect("a first sample");
        parser
    }

    #[test]
    fn an_rmc_sentence_makes_a_sample() {
        let sample = parser().absorb(&fix().rmc()).expect("a sample");

        assert_eq!(sample.latitude(), 50.5);
        assert_eq!(sample.longitude(), 8.5);
        assert_eq!(sample.t, fix().t());
    }

    /// Speed is the one field a receiver reports in a unit a sample does not use: 4.13 knots
    /// is 2.125 metres per second, and a predictor dividing a distance by knots would be
    /// wrong by a factor of two.
    #[test]
    fn a_speed_in_knots_becomes_metres_per_second() {
        let sample = parser().absorb(&fix().rmc()).expect("a sample");

        assert_near(sample.speed_mps, 4.13 * 1_852.0 / 3_600.0);
        assert_near(sample.heading_degrees, 79.94);
    }

    /// GGA carries no date, so nothing it says can be placed on a timeline on its own.
    #[test]
    fn a_gga_sentence_alone_makes_no_sample() {
        assert_eq!(parser().absorb(&fix().gga()), None);
    }

    /// Once an RMC has supplied the date, the accumulator keeps it, and a GGA fills in what
    /// RMC does not carry.
    #[test]
    fn a_gga_after_an_rmc_makes_a_sample_reporting_the_fix_quality() {
        let sample = fixed().absorb(&fix().gga()).expect("a sample");

        assert_eq!(sample.satellites, Some(6));
        assert_near(sample.hdop, 4.4);
        assert_near(sample.altitude_metres, 262.46);
    }

    #[test]
    fn a_later_sentence_moves_the_sample() {
        let sample = fixed().absorb(&later().gga()).expect("a sample");

        assert_eq!(sample.latitude(), 50.5001);
        assert_eq!(
            sample.t,
            Utc.with_ymd_and_hms(2026, 7, 29, 20, 43, 30).unwrap()
        );
    }

    #[test]
    fn a_stationary_rmc_with_no_course_still_makes_a_sample() {
        let stationary = Fix::at(20, 48, 58, 50.5, 8.5).with_speed_knots(0.08);

        let sample = parser().absorb(&stationary.rmc()).expect("a sample");

        assert_eq!(sample.latitude(), 50.5);
        assert_eq!(sample.heading_degrees, None);
    }

    /// The receiver reports its own doubt by dropping the position, and a sample without a
    /// position is not a sample. What the state machine still holds is its own business.
    #[test]
    fn a_void_sentence_makes_no_sample() {
        assert_eq!(fixed().absorb(RMC_VOID), None);
    }

    #[test]
    fn the_real_indoor_stream_makes_no_samples() {
        let mut parser = parser();

        for sentence in [RMC_VOID, GGA_NO_FIX, GSA_NO_FIX, SPLICED] {
            assert_eq!(parser.absorb(sentence), None, "{sentence}");
        }
    }

    #[test]
    fn a_spliced_sentence_makes_no_sample() {
        assert_eq!(fixed().absorb(SPLICED), None);
    }

    /// A well-formed later sentence whose checksum does not match its contents, so the move
    /// to 50.5001 must not be believed.
    #[test]
    fn a_corrupt_sentence_makes_no_sample() {
        assert_eq!(fixed().absorb(&with_bad_checksum(&later().gga())), None);
    }

    #[test]
    fn noise_on_the_line_does_not_panic() {
        assert_eq!(parser().absorb("\0\u{1}not a sentence"), None);
    }

    /// A dozen sentences a second arrive saying what the last one said. Reporting each as a
    /// fresh sample would have the predictor re-deciding what it has already decided.
    #[test]
    fn a_sentence_adding_nothing_makes_no_sample() {
        let mut parser = fixed();

        assert!(parser.absorb(&fix().gga()).is_some());
        assert_eq!(parser.absorb(&fix().gga()), None);
    }
}
