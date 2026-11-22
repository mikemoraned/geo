use nmea::Nmea;

use crate::sentence::Sentence;
use domain::{Precision, Sample};

const METRES_PER_SECOND_PER_KNOT: f64 = 1_852.0 / 3_600.0;

#[derive(Debug, Clone)]
pub struct Parser<P: Precision> {
    sentences: Nmea,
    last: Option<Sample<P>>,
}

impl<P: Precision> Default for Parser<P> {
    fn default() -> Self {
        Self {
            sentences: Nmea::default(),
            last: None,
        }
    }
}

impl<P: Precision> Parser<P> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn absorb(&mut self, sentence: &Sentence) -> Option<Sample<P>> {
        self.sentences.parse(sentence.as_str()).ok()?;

        let sample = self.sample()?;
        if self.last == Some(sample) {
            return None;
        }
        self.last = Some(sample);
        Some(sample)
    }

    fn sample(&self) -> Option<Sample<P>> {
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
