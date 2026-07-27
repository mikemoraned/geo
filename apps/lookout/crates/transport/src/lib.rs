//! Point-in-time extracts of Overture Maps into the bronze layer of the medallion store.
//!
//!   - [`overture`] — read one release, from the public bucket or a local mirror of it.
//!   - [`extract`] — write a country's rail, water and divisions from it into bronze.

pub mod extract;
pub mod overture;
