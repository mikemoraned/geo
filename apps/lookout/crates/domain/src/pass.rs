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

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a pass counts at most {max} samples within the radius, not {0}", max = u32::MAX)]
pub struct TooManySamples(usize);

impl Pass {
    pub fn new(
        session_id: SessionId,
        crossing_id: CrossingId,
        crossed_at: DateTime<Utc>,
        distance_metres: f64,
        samples_within: usize,
    ) -> Result<Self, TooManySamples> {
        Ok(Self {
            session_id,
            crossing_id,
            crossed_at,
            distance_metres,
            samples_within: samples_within
                .try_into()
                .map_err(|_| TooManySamples(samples_within))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass(samples_within: usize) -> Result<Pass, TooManySamples> {
        Pass::new(
            "a-session".parse().expect("a session id"),
            "a-crossing".parse().expect("a crossing id"),
            DateTime::from_timestamp(1_760_000_000, 0).expect("an instant"),
            10.0,
            samples_within,
        )
    }

    #[test]
    fn a_pass_counts_its_samples() {
        assert_eq!(pass(3).expect("a pass").samples_within, 3);
    }

    #[test]
    fn a_pass_refuses_more_samples_than_it_can_count() {
        let too_many = u32::MAX as usize + 1;

        assert_eq!(pass(too_many), Err(TooManySamples(too_many)));
    }
}
