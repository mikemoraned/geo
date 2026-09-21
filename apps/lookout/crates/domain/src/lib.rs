//! The things lookout handles, in types a device and the store can both hold.
//!
//! A crossing means the same thing wherever it is: derived from reference data, kept in the
//! store, packed into flash, fetched by a page, scanned against a fix. It is described once,
//! here, and everything else refers to it. What else belongs here, and what belongs instead
//! to whoever stores or sends one, is `README.md`.
//!
//! Nothing here uses arrow, a filesystem or a clock: this crate builds for Xtensa and for
//! wasm, and neither has any of the three. The store's schema — which datasets exist, how they
//! are partitioned, what columns they have — is `medallion-model`, which names the types here
//! rather than restating them.

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
pub mod train;

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
pub use train::TrainNumber;
