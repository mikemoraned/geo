use std::collections::VecDeque;
use std::fmt;
use std::time::{Duration, Instant};

use indicatif::BinaryBytes;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Rate {
    bytes_per_second: f64,
}

impl Rate {
    pub fn across(bytes: u64, duration: Duration) -> Option<Self> {
        let seconds = duration.as_secs_f64();
        (seconds > 0.0).then(|| Self {
            bytes_per_second: bytes as f64 / seconds,
        })
    }

    pub fn time_for(self, bytes: u64) -> Option<Duration> {
        (self.bytes_per_second > 0.0)
            .then(|| Duration::from_secs_f64(bytes as f64 / self.bytes_per_second))
    }
}

impl fmt::Display for Rate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}/s", BinaryBytes(self.bytes_per_second as u64))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RateWindow {
    length: Duration,
    samples: VecDeque<(Instant, u64)>,
}

impl RateWindow {
    pub(crate) fn new(length: Duration) -> Self {
        Self {
            length,
            samples: VecDeque::new(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.samples.clear();
    }

    pub(crate) fn record(&mut self, now: Instant, done: u64) {
        self.samples.push_back((now, done));
        while self
            .samples
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > self.length)
        {
            self.samples.pop_front();
        }
    }

    pub(crate) fn rate(&self) -> Option<Rate> {
        let (first_at, first_done) = self.samples.front()?;
        let (last_at, last_done) = self.samples.back()?;
        Rate::across(last_done - first_done, last_at.duration_since(*first_at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate(bytes: u64, seconds: u64) -> Rate {
        Rate::across(bytes, Duration::from_secs(seconds)).expect("a rate")
    }

    #[test]
    fn no_time_gives_no_rate() {
        assert_eq!(Rate::across(100, Duration::ZERO), None);
    }

    #[test]
    fn a_rate_is_the_bytes_over_the_time() {
        assert_eq!(rate(1000, 20), rate(50, 1));
    }

    #[test]
    fn the_time_for_some_bytes_is_their_share_of_the_rate() {
        assert_eq!(rate(50, 1).time_for(1000), Some(Duration::from_secs(20)));
    }

    #[test]
    fn a_rate_of_nothing_never_finishes() {
        assert_eq!(rate(0, 30).time_for(1000), None);
    }

    #[test]
    fn a_rate_prints_in_binary_units_per_second() {
        assert_eq!(rate(4 << 20, 1).to_string(), "4.00 MiB/s");
    }

    fn window_with(samples: &[(u64, u64)]) -> RateWindow {
        let start = Instant::now();
        let mut window = RateWindow::new(Duration::from_secs(60));
        for (seconds, done) in samples {
            window.record(start + Duration::from_secs(*seconds), *done);
        }
        window
    }

    fn per_second(bytes: u64) -> Option<Rate> {
        Rate::across(bytes, Duration::from_secs(1))
    }

    #[test]
    fn a_single_sample_gives_no_rate() {
        assert_eq!(window_with(&[(0, 100)]).rate(), None);
    }

    #[test]
    fn the_rate_is_the_bytes_gained_across_the_window() {
        assert_eq!(
            window_with(&[(0, 0), (10, 500), (20, 1000)]).rate(),
            per_second(50)
        );
    }

    #[test]
    fn a_file_landing_at_once_is_spread_across_the_window() {
        assert_eq!(
            window_with(&[(0, 0), (30, 0), (31, 600), (60, 600)]).rate(),
            per_second(10)
        );
    }

    #[test]
    fn samples_older_than_the_window_are_dropped() {
        assert_eq!(
            window_with(&[(0, 0), (1, 10_000), (70, 10_000), (130, 16_000)]).rate(),
            per_second(100)
        );
    }
}
