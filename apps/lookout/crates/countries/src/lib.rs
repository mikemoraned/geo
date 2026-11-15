mod areas;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;

pub use areas::{CountryAreas, CountryError, NewestExtract, Outline};
