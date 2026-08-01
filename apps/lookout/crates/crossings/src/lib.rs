//! Turn the silver water-crossings dataset into the flat point buffer the M5 device scans.
//!
//! The device holds every crossing in RAM and brute-force scans the lot against each GPS fix,
//! so what it needs is not a queryable dataset but a packed array of coordinates. Deriving
//! that is this crate's whole job.

pub mod bbox;
pub mod id;
pub mod pointset;
pub mod random;
pub mod silver;

pub use bbox::{Bbox, BboxError};
pub use id::{Collision, CrossingId};
pub use pointset::{FormatError, Point};
pub use silver::{Crossing, ReadError};
