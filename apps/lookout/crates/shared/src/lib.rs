//! Telemetry models shared by the `server` (which receives messages over a websocket
//! and queues them) and the `recorder` cli (which drains the queue and interprets
//! them).
//!
//! The wire model is a versioned [`Message`] enum — see the [`message`] module for
//! the on-the-wire shape and how the protocol version is carried. Sensor payloads
//! live in [`sensor`]; what a device says about itself is [`domain::DeviceInfo`].

pub mod message;
pub mod sensor;

pub use message::{AccelReading, GpsReading, Message, SessionStart, V0Message, V1Message};
pub use sensor::Accel;
