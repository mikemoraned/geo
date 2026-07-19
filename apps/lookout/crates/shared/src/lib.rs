//! Telemetry models shared by the `server` (which receives messages over a websocket
//! and queues them) and the `recorder` cli (which drains the queue and interprets
//! them).
//!
//! The wire model is a versioned [`Message`] enum — see the [`message`] module for
//! the on-the-wire shape and how the protocol version is carried. Sensor payloads
//! live in [`sensor`]; per-session device metadata in [`session`]. Spatial types
//! shared across crates live in [`bbox`].

pub mod bbox;
pub mod message;
pub mod sensor;
pub mod session;

pub use bbox::BBox;
pub use message::{AccelReading, GpsReading, Message, SessionStart, V0Message, V1Message};
pub use sensor::{Accel, Gps};
pub use session::{DeviceInfo, DeviceType};
