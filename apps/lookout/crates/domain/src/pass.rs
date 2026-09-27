use chrono::{DateTime, Utc};

use crate::crossing::CrossingId;
use crate::session::SessionId;

#[derive(Debug, Clone, PartialEq)]
pub struct Pass {
    pub session_id: SessionId,
    pub crossing_id: CrossingId,
    pub crossed_at: DateTime<Utc>,
    pub distance_metres: f64,
    pub samples_within: u32,
}
