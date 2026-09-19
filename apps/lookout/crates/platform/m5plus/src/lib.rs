//! The M5StickC PLUS2 shell: the pins, the peripherals, and what to do with them.
//!
//! Everything here is wiring. What a sentence means, what a voltage means, and what belongs on
//! the screen are all [`platform_core`]'s, where a laptop can test them — a crate depending on
//! `esp-idf-*` cannot be tested at all, which is what forces the split rather than a preference
//! for it.
//!
//! The board facts these modules carry are established by running code on this hardware, and
//! several of them contradict the vendor and community documentation. See
//! `apps/lookout/docs/device.md`.

pub mod battery;
pub mod gnss;
pub mod panel;
