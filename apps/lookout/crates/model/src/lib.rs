//! The things lookout handles, in types a device and the store can both hold.
//!
//! A fix means the same thing wherever it is: read from a phone's geolocation, sent over the
//! wire, kept in the store, replayed in a browser, predicted from on a board. It is described
//! once, here, and each of those refers to it rather than spelling out its own.
//!
//! Nothing here uses arrow, a filesystem or a clock: this crate builds for Xtensa and for
//! wasm, and neither has any of the three. The store's schema — which datasets exist, how they
//! are partitioned, what columns they have — is `medallion-model`, which depends on all three.

pub mod crossing;
pub mod gps;

pub use crossing::Crossing;
pub use gps::Gps;
