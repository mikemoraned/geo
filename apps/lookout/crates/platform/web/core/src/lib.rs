//! A counter core, standing in for the predictor while the path to the browser is built.
//!
//! It exists to be replaced. What it proves is everything around it — the bridge, the custom
//! element, the wasm build and the deploy — so it takes the shape the predictor will take: an
//! event carrying a time, a clock discipline that refuses a stale one, and a `Render` asked for
//! only where something moved.

mod app;
pub mod view;

pub use app::{Counter, Effect, Event, Model, ViewModel};
pub use view::Browser;
