//! A crossing met in a session.

use chrono::{DateTime, Utc};

use crate::crossing::CrossingId;
use crate::session::SessionId;

/// One crossing a session passed: which, when, and on what evidence.
///
/// The instant is that of the session's nearest sample to the crossing, which is as precise
/// as the recording gets — nothing observes the passing itself. [`Pass::distance_metres`] and
/// [`Pass::samples_within`] are what say how good that evidence is: a crossing matched by one
/// distant sample and one matched by twenty close ones are both passes, and a reader weighs
/// them.
///
/// What radius a run matched within is not here. A pass is what was found; the radius is how
/// hard it looked, and belongs with whoever records the run.
#[derive(Debug, Clone, PartialEq)]
pub struct Pass {
    pub session_id: SessionId,
    pub crossing_id: CrossingId,
    pub crossed_at: DateTime<Utc>,
    /// How far the nearest sample was from the crossing, in metres.
    pub distance_metres: f64,
    /// How many of the session's samples fell within the radius.
    pub samples_within: u32,
}
