use geo_types::CoordFloat;
use num_traits::FromPrimitive;

pub trait Precision: CoordFloat + FromPrimitive + Into<f64> {
    fn from_f64_rounded(value: f64) -> Self;
}

impl Precision for f32 {
    fn from_f64_rounded(value: f64) -> Self {
        value as f32
    }
}

impl Precision for f64 {
    fn from_f64_rounded(value: f64) -> Self {
        value
    }
}
