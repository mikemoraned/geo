use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use object_store::path::Path as ObjectPath;

const THEME_PART: &str = "theme=";

const OTHER: &str = "other";

const OVERALL: &str = "release";

const TEMPLATE: &str = "{spinner} {prefix:>15} [{bar:30}] {bytes:>10} / {total_bytes:<10} {binary_bytes_per_sec:>12}  {eta} left";

const TICK: Duration = Duration::from_millis(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Display {
    Bars,
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Theme(String);

impl Theme {
    pub fn of(within_prefix: &ObjectPath) -> Self {
        let name = within_prefix
            .parts()
            .find_map(|part| part.as_ref().strip_prefix(THEME_PART).map(str::to_string));
        Self(name.unwrap_or_else(|| OTHER.to_string()))
    }
}

impl fmt::Display for Theme {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug)]
pub struct Progress {
    overall: ProgressBar,
    themes: BTreeMap<Theme, ProgressBar>,
}

impl Progress {
    pub fn new(files: &[(Theme, u64)], display: Display) -> Self {
        let multi = MultiProgress::with_draw_target(match display {
            Display::Bars => ProgressDrawTarget::stderr(),
            Display::Hidden => ProgressDrawTarget::hidden(),
        });
        let mut totals: BTreeMap<Theme, u64> = BTreeMap::new();
        for (theme, size) in files {
            *totals.entry(theme.clone()).or_default() += size;
        }
        let themes = totals
            .into_iter()
            .map(|(theme, total)| {
                let bar = multi.add(bar(total, theme.to_string()));
                (theme, bar)
            })
            .collect();
        let overall = multi.add(bar(
            files.iter().map(|(_, size)| size).sum(),
            OVERALL.to_string(),
        ));
        Self { overall, themes }
    }

    pub fn advance(&self, theme: &Theme, bytes: u64) {
        self.themes.get(theme).inspect(|bar| bar.inc(bytes));
        self.overall.inc(bytes);
    }

    pub fn skip(&self, theme: &Theme, bytes: u64) {
        self.themes.get(theme).inspect(|bar| bar.dec_length(bytes));
        self.overall.dec_length(bytes);
    }

    pub fn finish(&self) {
        self.themes.values().for_each(ProgressBar::finish);
        self.overall.finish();
    }
}

fn bar(total: u64, prefix: String) -> ProgressBar {
    let style = ProgressStyle::with_template(TEMPLATE)
        .expect("TEMPLATE is a constant indicatif accepts")
        .progress_chars("=> ");
    let bar = ProgressBar::new(total)
        .with_style(style)
        .with_prefix(prefix);
    bar.enable_steady_tick(TICK);
    bar
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(name: &str) -> Theme {
        Theme(name.to_string())
    }

    fn files() -> Vec<(Theme, u64)> {
        vec![
            (theme("base"), 100),
            (theme("base"), 50),
            (theme("divisions"), 30),
        ]
    }

    fn done_and_total(bar: &ProgressBar) -> (u64, u64) {
        (bar.position(), bar.length().expect("a bar with a length"))
    }

    #[test]
    fn a_file_belongs_to_the_theme_its_path_names() {
        let path = ObjectPath::from("2026-09-23.1/theme=base/type=water/part-0.parquet");

        assert_eq!(Theme::of(&path), theme("base"));
    }

    #[test]
    fn a_file_outside_any_theme_belongs_to_other() {
        assert_eq!(
            Theme::of(&ObjectPath::from("2026-09-23.1/README")),
            theme("other")
        );
    }

    #[test]
    fn each_theme_totals_the_bytes_of_its_files() {
        let progress = Progress::new(&files(), Display::Hidden);

        assert_eq!(done_and_total(&progress.themes[&theme("base")]), (0, 150));
        assert_eq!(
            done_and_total(&progress.themes[&theme("divisions")]),
            (0, 30)
        );
        assert_eq!(done_and_total(&progress.overall), (0, 180));
    }

    #[test]
    fn advancing_a_theme_advances_the_release_too() {
        let progress = Progress::new(&files(), Display::Hidden);

        progress.advance(&theme("base"), 40);

        assert_eq!(done_and_total(&progress.themes[&theme("base")]), (40, 150));
        assert_eq!(done_and_total(&progress.overall), (40, 180));
    }

    #[test]
    fn a_skipped_file_leaves_the_work_rather_than_counting_as_done() {
        let progress = Progress::new(&files(), Display::Hidden);

        progress.skip(&theme("divisions"), 30);

        assert_eq!(
            done_and_total(&progress.themes[&theme("divisions")]),
            (0, 0)
        );
        assert_eq!(done_and_total(&progress.overall), (0, 150));
    }
}
