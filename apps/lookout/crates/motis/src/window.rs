use chrono::{DateTime, Duration, Utc};
use geo::{BoundingRect, Scale};
use geo_types::{MultiPoint, Point, Rect};

pub const DEFAULT_MAX_AGE: Duration = Duration::minutes(30);

const BUFFER_FACTOR: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub at: DateTime<Utc>,
    pub point: Point<f64>,
}

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
    pub fn new(max_age: Duration) -> Self {
        Self {
            max_age,
            positions: Vec::new(),
        }
    }

    pub fn ingest(&mut self, position: Position) {
        self.positions.push(position);
    }

    pub fn prune(&mut self, now: DateTime<Utc>) {
        let cutoff = now - self.max_age;
        self.positions.retain(|p| p.at >= cutoff);
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn bbox(&self) -> Option<Rect<f64>> {
        let points: MultiPoint<f64> = self.positions.iter().map(|p| p.point).collect();
        points.bounding_rect()
    }

    pub fn buffered_bbox(&self) -> Option<Rect<f64>> {
        Some(self.bbox()?.scale(BUFFER_FACTOR))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo_types::Coord;
    use proptest::prelude::*;

    fn pos(t: i64, lat: f64, lon: f64) -> Position {
        Position {
            at: DateTime::from_timestamp_millis(t).expect("an instant"),
            point: Point::new(lon, lat),
        }
    }

    fn at(t: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(t).expect("an instant")
    }

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
        let w = PositionWindow::new(Duration::minutes(30));
        assert!(w.is_empty());
        assert_eq!(w.bbox(), None);
        assert_eq!(w.buffered_bbox(), None);
    }

    #[test]
    fn bbox_spans_all_held_positions() {
        let mut w = PositionWindow::new(Duration::minutes(30));
        w.ingest(pos(0, 50.0, 8.0));
        w.ingest(pos(1, 51.0, 9.0));
        w.ingest(pos(2, 49.5, 8.5));
        assert_eq!(w.bbox(), Some(rect(49.5, 51.0, 8.0, 9.0)));
    }

    #[test]
    fn single_position_is_a_point_box() {
        let mut w = PositionWindow::new(Duration::minutes(30));
        w.ingest(pos(0, 50.0, 8.0));
        assert_eq!(w.bbox(), Some(rect(50.0, 50.0, 8.0, 8.0)));
    }

    #[test]
    fn buffered_box_doubles_each_dimension_about_centre() {
        let mut w = PositionWindow::new(Duration::minutes(30));
        w.ingest(pos(0, 50.0, 8.0));
        w.ingest(pos(1, 52.0, 12.0));
        let tight = rect(50.0, 52.0, 8.0, 12.0);
        let doubled_about_centre = rect(49.0, 53.0, 6.0, 14.0);

        assert_eq!(w.bbox(), Some(tight));
        assert_eq!(w.buffered_bbox(), Some(doubled_about_centre));
    }

    #[test]
    fn prune_drops_positions_older_than_max_age() {
        let max_age = Duration::seconds(60);
        let now = 60_000;
        let mut w = PositionWindow::new(max_age);
        let at_the_cutoff = pos(now - 60_000, 50.0, 8.0);
        let within_the_window = pos(now - 30_000, 51.0, 9.0);
        let older_than_the_cutoff = pos(now - 90_000, 40.0, 1.0);

        for position in [at_the_cutoff, within_the_window, older_than_the_cutoff] {
            w.ingest(position);
        }
        w.prune(at(now));

        assert_eq!(w.len(), 2);
        assert_eq!(w.bbox(), Some(rect(50.0, 51.0, 8.0, 9.0)));
    }

    prop_compose! {
        fn positions()(
            v in prop::collection::vec(
                (0i64..4_102_444_800_000, -90.0f64..=90.0, -180.0f64..=180.0),
                1..20,
            )
        ) -> Vec<Position> {
            v.into_iter().map(|(t, lat, lon)| pos(t, lat, lon)).collect()
        }
    }

    proptest! {
        #[test]
        fn buffered_box_contains_the_tight_box(ps in positions()) {
            let mut w = PositionWindow::new(Duration::minutes(30));
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
        fn pruning_is_monotonic(ps in positions(), now in 0i64..4_102_444_800_000) {
            let mut w = PositionWindow::new(Duration::minutes(30));
            for p in ps {
                w.ingest(p);
            }
            let before = w.len();
            w.prune(at(now));
            let after_first = w.len();
            w.prune(at(now));
            let after_second = w.len();
            prop_assert!(after_first <= before);
            prop_assert_eq!(after_first, after_second);
        }
    }
}
