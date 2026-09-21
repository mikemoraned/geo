//! The things lookout handles, in types a device and the store can both hold.
//!
//! A fix means the same thing wherever it is: read from a phone's geolocation, sent over the
//! wire, kept in the store, replayed in a browser, predicted from on a board. So does a
//! crossing, and what a crossing is called. Each is described once, here, and everything else
//! refers to it rather than spelling out its own.
//!
//! Nothing here uses arrow, a filesystem or a clock: this crate builds for Xtensa and for
//! wasm, and neither has any of the three. The store's schema — which datasets exist, how they
//! are partitioned, what columns they have — is `medallion-model`, which depends on all three.
//! It names the types here rather than restating them, so a device and a store hold one
//! description of a fix or a crossing between them.

pub mod bbox;
pub mod crossing;
pub mod device;
pub mod gps;
pub mod name;
pub mod pass;
pub mod position;
pub mod precision;
pub mod sample;
pub mod session;

pub use bbox::{Bbox, BboxError};
pub use crossing::{Crossing, CrossingCompact, CrossingCompactId, CrossingId};
pub use device::{DeviceId, DeviceInfo, DeviceType, EmptyDeviceId};
pub use gps::Gps;
pub use name::NameError;
pub use pass::Pass;
pub use position::{CoordinateError, position};
pub use precision::Precision;
pub use sample::Sample;
pub use session::{SessionId, StartedBy};
