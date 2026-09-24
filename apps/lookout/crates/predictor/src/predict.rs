use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use domain::{CrossingCompactId, Precision, Sample};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Event<P: Precision> {
    Sampled(Sample<P>),
    Elapsed(DateTime<Utc>),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Prediction<P: Precision> {
    pub crossing_compact_id: CrossingCompactId,
    pub metres: P,
    pub at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ObserveError {
    #[error("an event at {at} is behind the clock at {now}")]
    OutOfOrder {
        now: DateTime<Utc>,
        at: DateTime<Utc>,
    },
}

pub trait Predict<P: Precision> {
    fn observe(&mut self, event: Event<P>) -> Result<(), ObserveError>;

    fn predictions(&self) -> &[Prediction<P>];
}
