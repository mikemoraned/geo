//! What the M5StickC PLUS2 makes of the core: the crossings it carries, and the panel it
//! draws.
//!
//! Where anything is and what is about to be crossed is [`platform_core`]'s, and shared with
//! every other shell. What only this board has is here: the set built into its flash, and a
//! 135-pixel screen's worth of strings. [`Device`] is the two of them, as a
//! [`platform_core::Standalone`] platform: it carries its crossings, so the core only ever
//! asks it to draw.
//!
//! Separate from the shell beside it because the shell cannot be tested. A crate depending on
//! `esp-idf-*` does not compile for the host at all. So everything here — all of what the
//! board decides, minus the pins — sits where a laptop can run it.

pub mod carried;
pub mod device;
pub mod panel;

pub use device::Device;
pub use panel::{NEAREST_ON_SCREEN, ViewModel};
