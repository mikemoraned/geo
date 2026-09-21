use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use domain::{CrossingCompactId, Precision, Sample};

/// What a predictor is told.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Event<P: Precision> {
    Sampled(Sample<P>),
    Elapsed(DateTime<Utc>),
}

/// One crossing a predictor expects us to reach.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Prediction<P: Precision> {
    pub crossing: CrossingCompactId,
    /// The straight-line distance from the latest fix. Crow-flies: the track's own geometry
    /// plays no part, so a bend or a river meander puts a crossing nearer than the rails do.
    pub metres: P,
    /// When we reach it at the speed of the latest fix, absent where there is no speed to
    /// divide by.
    ///
    /// An instant rather than a countdown, so it stays true while the clock advances between
    /// fixes. A shell wanting a countdown subtracts the time it is showing it at.
    pub at: Option<DateTime<Utc>>,
}

/// Why an event was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ObserveError {
    /// The event is dated before the clock, which only goes forwards. It marks a sample that
    /// arrived late, a shell that read the time before the fix it is handing over, or a
    /// receiver that has jumped. None of the three is worth predicting from.
    #[error("an event at {at} is behind the clock at {now}")]
    OutOfOrder {
        now: DateTime<Utc>,
        at: DateTime<Utc>,
    },
}

/// Events in, predictions out: everything a shell needs from a predictor, and no more.
pub trait Predict<P: Precision> {
    /// Takes one event and transitions.
    ///
    /// An event the predictor refuses changes nothing: the clock, the predictions and the
    /// trend are all left as the last accepted event left them.
    fn observe(&mut self, event: Event<P>) -> Result<(), ObserveError>;

    /// The crossings it predicts we reach, nearest first.
    fn predictions(&self) -> &[Prediction<P>];
}
