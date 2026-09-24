//! What a browser makes of the core: the crossings it fetches, and the view it draws.
//!
//! Where anything is and what is about to be crossed is [`platform_core`]'s, and shared with
//! the device. What only a browser has is here: a set arriving over the network rather than
//! sitting in flash, and positions rather than pixels for whatever draws them.

pub mod view;

pub use view::{Browser, Here, Predicted, ViewModel};
