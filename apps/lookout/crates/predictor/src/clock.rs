use chrono::{DateTime, Utc};

use domain::Precision;

use crate::predict::Event;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("an event at {at} is behind the clock at {now}")]
pub struct OutOfOrder {
    pub now: DateTime<Utc>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Clock {
    now: Option<DateTime<Utc>>,
    sampled: Option<DateTime<Utc>>,
}

impl Clock {
    pub fn now(&self) -> Option<DateTime<Utc>> {
        self.now
    }

    pub fn observe<P: Precision>(&mut self, event: &Event<P>) -> Result<DateTime<Utc>, OutOfOrder> {
        match event {
            Event::Sampled(sample) => self.sampled(sample.t),
            Event::Elapsed(at) => self.elapsed(*at),
        }
    }

    pub fn sampled(&mut self, at: DateTime<Utc>) -> Result<DateTime<Utc>, OutOfOrder> {
        if let Some(last) = self.sampled
            && at < last
        {
            return Err(OutOfOrder { now: last, at });
        }
        self.sampled = Some(at);
        self.now = Some(self.now.map_or(at, |now| now.max(at)));
        Ok(at)
    }

    pub fn elapsed(&mut self, at: DateTime<Utc>) -> Result<DateTime<Utc>, OutOfOrder> {
        if let Some(now) = self.now
            && at < now
        {
            return Err(OutOfOrder { now, at });
        }
        self.now = Some(at);
        Ok(at)
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;

    fn instant(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
            + TimeDelta::seconds(seconds)
    }

    #[test]
    fn a_clock_starts_at_no_time() {
        assert_eq!(Clock::default().now(), None);
    }

    #[test]
    fn time_elapsing_moves_the_clock_to_it() {
        let mut clock = Clock::default();

        clock.elapsed(instant(30)).expect("a time in order");

        assert_eq!(clock.now(), Some(instant(30)));
    }

    #[test]
    fn a_time_behind_the_clock_is_refused_and_leaves_the_clock() {
        let mut clock = Clock::default();
        clock.elapsed(instant(30)).expect("a time in order");

        let refused = clock.elapsed(instant(10));

        assert_eq!(
            refused,
            Err(OutOfOrder {
                now: instant(30),
                at: instant(10)
            })
        );
        assert_eq!(clock.now(), Some(instant(30)));
    }

    #[test]
    fn a_sample_behind_the_clock_but_ahead_of_the_last_sample_is_taken() {
        let mut clock = Clock::default();
        clock.sampled(instant(0)).expect("a sample in order");
        clock.elapsed(instant(60)).expect("a time in order");

        assert_eq!(clock.sampled(instant(10)), Ok(instant(10)));
        assert_eq!(
            clock.now(),
            Some(instant(60)),
            "the clock stays where the later of the two put it"
        );
    }

    #[test]
    fn a_sample_behind_the_last_sample_is_refused() {
        let mut clock = Clock::default();
        clock.sampled(instant(30)).expect("a sample in order");

        assert_eq!(
            clock.sampled(instant(10)),
            Err(OutOfOrder {
                now: instant(30),
                at: instant(10)
            })
        );
    }

    #[test]
    fn a_sample_ahead_of_the_clock_moves_it() {
        let mut clock = Clock::default();
        clock.elapsed(instant(30)).expect("a time in order");

        clock.sampled(instant(40)).expect("a sample in order");

        assert_eq!(clock.now(), Some(instant(40)));
    }
}
