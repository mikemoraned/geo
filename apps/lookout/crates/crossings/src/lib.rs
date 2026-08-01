//! The ground truth: which water crossings each recorded session actually passed.
//!
//! This is the join of two things neither crate below it owns — the sessions derived from the
//! telemetry, and the crossings derived from the reference data — so it lives on its own
//! rather than in either.
//!
//!   - [`matching`] — the rule: a crossing is passed when a sample comes within the radius.
//!   - [`silver`] — reading both datasets and writing the `session_crossing` dataset.

pub mod matching;
pub mod silver;
