#![cfg(any(test, feature = "fixtures"))]

use geo_types::{Coord, Point};

pub const BERLIN: Point<f64> = Point(Coord {
    x: 13.404954,
    y: 52.520008,
});
pub const DRESDEN_HBF: Point<f64> = Point(Coord {
    x: 13.7322,
    y: 51.0403,
});
pub const EDINBURGH: Point<f64> = Point(Coord {
    x: -3.188267,
    y: 55.953251,
});
pub const FRANKFURT: Point<f64> = Point(Coord {
    x: 8.682127,
    y: 50.110924,
});
pub const LONDON: Point<f64> = Point(Coord {
    x: -0.127758,
    y: 51.507351,
});
pub const MUNICH: Point<f64> = Point(Coord {
    x: 11.581981,
    y: 48.135125,
});
