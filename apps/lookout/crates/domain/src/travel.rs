use chrono::TimeDelta;
use geo::{Distance, Euclidean};
use geo_types::{LineString, Point};

pub fn path_through(points: impl IntoIterator<Item = Point<f64>>) -> LineString<f64> {
    let coords: Vec<_> = points.into_iter().map(|point| point.0).collect();
    match coords.as_slice() {
        [only] => LineString::new(vec![*only, *only]),
        _ => LineString::new(coords),
    }
}

pub fn implied_speed(from: Point<f64>, to: Point<f64>, took: TimeDelta) -> f64 {
    Euclidean.distance(from, to) / took.as_seconds_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_runs_through_each_point_in_turn() {
        let path = path_through([
            Point::new(8.5, 50.1),
            Point::new(8.6, 50.1),
            Point::new(8.6, 50.2),
        ]);

        assert_eq!(
            path,
            LineString::from(vec![(8.5, 50.1), (8.6, 50.1), (8.6, 50.2)])
        );
    }

    #[test]
    fn a_path_through_one_point_runs_from_it_to_itself() {
        let path = path_through([Point::new(8.5, 50.1)]);

        assert_eq!(path, LineString::from(vec![(8.5, 50.1), (8.5, 50.1)]));
    }

    #[test]
    fn the_implied_speed_is_metres_per_second_between_projected_points() {
        let speed = implied_speed(
            Point::new(500_000.0, 5_800_000.0),
            Point::new(500_300.0, 5_800_400.0),
            TimeDelta::seconds(10),
        );

        assert_eq!(speed, 50.0);
    }
}
