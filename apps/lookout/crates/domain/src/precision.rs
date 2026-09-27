use geo_types::CoordFloat;
use num_traits::FromPrimitive;

pub trait Precision: CoordFloat + FromPrimitive {}

impl<P: CoordFloat + FromPrimitive> Precision for P {}
