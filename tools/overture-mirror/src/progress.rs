use std::fmt;
use std::time::{Duration, Instant};

use indicatif::style::ProgressTracker;
use indicatif::{
    HumanDuration, ProgressBar, ProgressDrawTarget, ProgressFinish, ProgressState, ProgressStyle,
};

use crate::rate::RateWindow;

const TEMPLATE: &str =
    "{spinner} {prefix:<8} [{bar:40}] {bytes:>10} / {total_bytes:<10} {rate:>12}  {left}";

const TICK: Duration = Duration::from_millis(120);

const RATE_WINDOW_LENGTH: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
struct WindowedRate(RateWindow);

impl ProgressTracker for WindowedRate {
    fn clone_box(&self) -> Box<dyn ProgressTracker> {
        Box::new(self.clone())
    }

    fn tick(&mut self, state: &ProgressState, now: Instant) {
        self.0.record(now, state.pos());
    }

    fn reset(&mut self, _state: &ProgressState, _now: Instant) {
        self.0.clear();
    }

    fn write(&self, _state: &ProgressState, w: &mut dyn fmt::Write) {
        let _ = write!(w, "{}", self.0.rate().unwrap_or_default());
    }
}

#[derive(Debug, Clone)]
struct WindowedTimeLeft(RateWindow);

impl ProgressTracker for WindowedTimeLeft {
    fn clone_box(&self) -> Box<dyn ProgressTracker> {
        Box::new(self.clone())
    }

    fn tick(&mut self, state: &ProgressState, now: Instant) {
        self.0.record(now, state.pos());
    }

    fn reset(&mut self, _state: &ProgressState, _now: Instant) {
        self.0.clear();
    }

    fn write(&self, state: &ProgressState, w: &mut dyn fmt::Write) {
        let remaining = state.len().unwrap_or_default().saturating_sub(state.pos());
        let _ = match self.0.rate().and_then(|rate| rate.time_for(remaining)) {
            Some(left) => write!(w, "{} left", HumanDuration(left)),
            None => write!(w, "time left unknown"),
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Display {
    Bars,
    Hidden,
}

#[derive(Debug)]
pub struct Progress {
    bar: ProgressBar,
}

impl Progress {
    pub fn new(stage: &'static str, total: u64, display: Display) -> Self {
        let target = match display {
            Display::Bars => ProgressDrawTarget::stderr(),
            Display::Hidden => ProgressDrawTarget::hidden(),
        };
        let style = ProgressStyle::with_template(TEMPLATE)
            .expect("TEMPLATE is a constant indicatif accepts")
            .progress_chars("=> ")
            .with_key("rate", WindowedRate(RateWindow::new(RATE_WINDOW_LENGTH)))
            .with_key(
                "left",
                WindowedTimeLeft(RateWindow::new(RATE_WINDOW_LENGTH)),
            );
        let bar = ProgressBar::with_draw_target(Some(total), target)
            .with_style(style)
            .with_prefix(stage)
            .with_finish(ProgressFinish::Abandon);
        bar.enable_steady_tick(TICK);
        Self { bar }
    }

    pub fn advance(&self, bytes: u64) {
        self.bar.inc(bytes);
    }

    pub fn skip(&self, bytes: u64) {
        self.bar.dec_length(bytes);
    }

    pub fn finish(&self) {
        self.bar.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done_and_total(progress: &Progress) -> (u64, u64) {
        (
            progress.bar.position(),
            progress.bar.length().expect("a bar with a length"),
        )
    }

    #[test]
    fn advancing_counts_bytes_as_done() {
        let progress = Progress::new("testing", 180, Display::Hidden);

        progress.advance(40);

        assert_eq!(done_and_total(&progress), (40, 180));
    }

    #[test]
    fn skipped_bytes_leave_the_work_rather_than_counting_as_done() {
        let progress = Progress::new("testing", 180, Display::Hidden);

        progress.skip(30);

        assert_eq!(done_and_total(&progress), (0, 150));
    }

    #[test]
    fn finishing_completes_the_bar() {
        let progress = Progress::new("testing", 180, Display::Hidden);
        progress.advance(40);

        progress.finish();

        assert_eq!(done_and_total(&progress), (180, 180));
    }
}
