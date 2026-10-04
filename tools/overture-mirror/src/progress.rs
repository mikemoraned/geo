use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

const TEMPLATE: &str =
    "{spinner} [{bar:40}] {bytes:>10} / {total_bytes:<10} {binary_bytes_per_sec:>12}  {eta} left";

const TICK: Duration = Duration::from_millis(120);

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
    pub fn new(total: u64, display: Display) -> Self {
        let target = match display {
            Display::Bars => ProgressDrawTarget::stderr(),
            Display::Hidden => ProgressDrawTarget::hidden(),
        };
        let style = ProgressStyle::with_template(TEMPLATE)
            .expect("TEMPLATE is a constant indicatif accepts")
            .progress_chars("=> ");
        let bar = ProgressBar::with_draw_target(Some(total), target).with_style(style);
        bar.enable_steady_tick(TICK);
        Self { bar }
    }

    pub fn advance(&self, bytes: u64) {
        self.bar.inc(bytes);
    }

    pub fn skip(&self, bytes: u64) {
        self.bar.dec_length(bytes);
    }

    pub fn close<T, E>(&self, outcome: Result<T, E>) -> Result<T, E> {
        match outcome {
            Ok(_) => self.bar.finish(),
            Err(_) => self.bar.abandon(),
        }
        outcome
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
        let progress = Progress::new(180, Display::Hidden);

        progress.advance(40);

        assert_eq!(done_and_total(&progress), (40, 180));
    }

    #[test]
    fn skipped_bytes_leave_the_work_rather_than_counting_as_done() {
        let progress = Progress::new(180, Display::Hidden);

        progress.skip(30);

        assert_eq!(done_and_total(&progress), (0, 150));
    }

    #[test]
    fn closing_on_success_completes_the_bar() {
        let progress = Progress::new(180, Display::Hidden);
        progress.advance(40);

        let outcome: Result<(), ()> = progress.close(Ok(()));

        assert_eq!(outcome, Ok(()));
        assert_eq!(done_and_total(&progress), (180, 180));
    }

    #[test]
    fn closing_on_failure_leaves_the_bar_where_it_stopped() {
        let progress = Progress::new(180, Display::Hidden);
        progress.advance(40);

        let outcome: Result<(), ()> = progress.close(Err(()));

        assert_eq!(outcome, Err(()));
        assert_eq!(done_and_total(&progress), (40, 180));
    }
}
