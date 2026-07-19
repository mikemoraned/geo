//! A rolling window of recent GPS positions and the bounding box they span. The poll
//! loop feeds it fixes as they arrive, prunes ones older than `max_age` relative to a
//! supplied `now`, and queries Motis for trips within a buffered box around what remains.

use std::time::Duration;

use geo_types::{Coord, Rect};

/// Age beyond which positions are pruned from the window, unless overridden.
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(30 * 60);

/// Factor the tight box is scaled by to give Motis some margin around the GPS trace.
const BUFFER_FACTOR: f64 = 2.0;

/// A timestamped GPS position. `t` is epoch milliseconds, matching the wire model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub t: i64,
    pub lat: f64,
    pub lon: f64,
}

/// A rolling set of recent positions, pruned by age relative to a supplied `now`.
#[derive(Debug, Clone)]
pub struct PositionWindow {
    max_age: Duration,
    positions: Vec<Position>,
}

impl Default for PositionWindow {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_AGE)
    }
}

impl PositionWindow {
    /// A window that retains positions younger than `max_age`.
    pub fn new(max_age: Duration) -> Self {
        Self {
            max_age,
            positions: Vec::new(),
        }
    }

    /// Add a position to the window.
    pub fn ingest(&mut self, position: Position) {
        self.positions.push(position);
    }

    /// Drop positions older than `max_age` relative to `now` (epoch milliseconds).
    pub fn prune(&mut self, now: i64) {
        let age_ms = i64::try_from(self.max_age.as_millis()).unwrap_or(i64::MAX);
        let cutoff = now.saturating_sub(age_ms);
        self.positions.retain(|p| p.t >= cutoff);
    }

    /// Number of positions currently held.
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    /// Whether the window holds no positions.
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// The tight bounding box of the held positions (a lat/lon [`Rect`], `x` = lon,
    /// `y` = lat), or `None` when empty.
    pub fn bbox(&self) -> Option<Rect<f64>> {
        let first = self.positions.first()?;
        let (mut min_lat, mut max_lat) = (first.lat, first.lat);
        let (mut min_lon, mut max_lon) = (first.lon, first.lon);
        for p in &self.positions {
            min_lat = min_lat.min(p.lat);
            max_lat = max_lat.max(p.lat);
            min_lon = min_lon.min(p.lon);
            max_lon = max_lon.max(p.lon);
        }
        Some(Rect::new(
            Coord {
                x: min_lon,
                y: min_lat,
            },
            Coord {
                x: max_lon,
                y: max_lat,
            },
        ))
    }

    /// The tight box with each dimension doubled about its centre, or `None` when empty.
    pub fn buffered_bbox(&self) -> Option<Rect<f64>> {
        Some(scaled_about_centre(&self.bbox()?, BUFFER_FACTOR))
    }
}

/// `rect` scaled about its centre by `factor`: each dimension's span is multiplied by
/// `factor`, the centre held fixed. Each side is extended by `(factor - 1) * span/2` — a
/// non-negative grow for `factor >= 1`, so the result always contains `rect` even under
/// floating-point rounding.
fn scaled_about_centre(rect: &Rect<f64>, factor: f64) -> Rect<f64> {
    let grow_x = rect.width() * (factor - 1.0) / 2.0;
    let grow_y = rect.height() * (factor - 1.0) / 2.0;
    Rect::new(
        Coord {
            x: rect.min().x - grow_x,
            y: rect.min().y - grow_y,
        },
        Coord {
            x: rect.max().x + grow_x,
            y: rect.max().y + grow_y,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn pos(t: i64, lat: f64, lon: f64) -> Position {
        Position { t, lat, lon }
    }

    /// A lat/lon [`Rect`] from the extent, keeping tests in `(lat, lon)` reading order.
    fn rect(min_lat: f64, max_lat: f64, min_lon: f64, max_lon: f64) -> Rect<f64> {
        Rect::new(
            Coord {
                x: min_lon,
                y: min_lat,
            },
            Coord {
                x: max_lon,
                y: max_lat,
            },
        )
    }

    #[test]
    fn empty_window_has_no_bbox() {
        let w = PositionWindow::new(Duration::from_secs(1800));
        assert!(w.is_empty());
        assert_eq!(w.bbox(), None);
        assert_eq!(w.buffered_bbox(), None);
    }

    #[test]
    fn bbox_spans_all_held_positions() {
        let mut w = PositionWindow::new(Duration::from_secs(1800));
        w.ingest(pos(0, 50.0, 8.0));
        w.ingest(pos(1, 51.0, 9.0));
        w.ingest(pos(2, 49.5, 8.5));
        assert_eq!(w.bbox(), Some(rect(49.5, 51.0, 8.0, 9.0)));
    }

    #[test]
    fn single_position_is_a_point_box() {
        let mut w = PositionWindow::new(Duration::from_secs(1800));
        w.ingest(pos(0, 50.0, 8.0));
        assert_eq!(w.bbox(), Some(rect(50.0, 50.0, 8.0, 8.0)));
    }

    #[test]
    fn buffered_box_doubles_each_dimension_about_centre() {
        let mut w = PositionWindow::new(Duration::from_secs(1800));
        w.ingest(pos(0, 50.0, 8.0));
        w.ingest(pos(1, 52.0, 12.0));
        // tight: lat 50..52 (span 2, centre 51), lon 8..12 (span 4, centre 10).
        // doubled about centre: lat 49..53, lon 6..14.
        assert_eq!(w.buffered_bbox(), Some(rect(49.0, 53.0, 6.0, 14.0)));
    }

    #[test]
    fn prune_drops_positions_older_than_max_age() {
        let mut w = PositionWindow::new(Duration::from_secs(60));
        w.ingest(pos(0, 50.0, 8.0)); // 60s before now → boundary, kept
        w.ingest(pos(30_000, 51.0, 9.0)); // 30s before now → kept
        w.ingest(pos(-30_000, 40.0, 1.0)); // 90s before now → dropped
        w.prune(60_000);
        assert_eq!(w.len(), 2);
        assert_eq!(w.bbox(), Some(rect(50.0, 51.0, 8.0, 9.0)));
    }

    prop_compose! {
        fn positions()(
            v in prop::collection::vec(
                (any::<i64>(), -90.0f64..=90.0, -180.0f64..=180.0),
                1..20,
            )
        ) -> Vec<Position> {
            v.into_iter().map(|(t, lat, lon)| pos(t, lat, lon)).collect()
        }
    }

    proptest! {
        #[test]
        fn buffered_box_contains_the_tight_box(ps in positions()) {
            let mut w = PositionWindow::new(Duration::from_secs(1800));
            for p in ps {
                w.ingest(p);
            }
            let tight = w.bbox().unwrap();
            let buffered = w.buffered_bbox().unwrap();
            prop_assert!(buffered.min().y <= tight.min().y);
            prop_assert!(buffered.max().y >= tight.max().y);
            prop_assert!(buffered.min().x <= tight.min().x);
            prop_assert!(buffered.max().x >= tight.max().x);
        }

        #[test]
        fn pruning_is_monotonic(ps in positions(), now in any::<i64>()) {
            let mut w = PositionWindow::new(Duration::from_secs(1800));
            for p in ps {
                w.ingest(p);
            }
            let before = w.len();
            w.prune(now);
            let after_first = w.len();
            w.prune(now);
            let after_second = w.len();
            // pruning never grows the window, and re-pruning at the same `now` is stable.
            prop_assert!(after_first <= before);
            prop_assert_eq!(after_first, after_second);
        }
    }
}
