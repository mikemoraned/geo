//! What a predictor is, as an interface: events in, predictions out.
//!
//! Two traits, because a shell needs two different amounts. [`Predict`] is what every shell
//! needs and stays small enough that a second implementation is worth writing. [`Trending`]
//! is what a panel with room to spare can also show, kept apart so that needing it is a
//! choice.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::crossing::CrossingId;
use crate::measure::Measure;
use crate::sample::Sample;

/// What a predictor is told.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Event<T: Measure> {
    /// A fix. It carries its own timestamp, so it advances the clock as well as moving the
    /// position.
    Sampled(Sample<T>),
    /// Time passing with no fix, so a predictor can tell a stale answer from a fresh one.
    Elapsed(DateTime<Utc>),
}

/// One crossing a predictor expects us to reach.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Prediction<T: Measure> {
    pub crossing: CrossingId,
    /// The straight-line distance from the last fix. Crow-flies: the track's own geometry
    /// plays no part, so a bend or a river meander puts a crossing nearer than the rails do.
    pub metres: T,
    /// When we reach it at the speed of the last fix, absent where there is no speed to
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
pub trait Predict<T: Measure> {
    /// Takes one event and transitions.
    ///
    /// An event the predictor refuses changes nothing: the clock, the predictions and the
    /// trend are all left as the last accepted event left them.
    fn observe(&mut self, event: Event<T>) -> Result<(), ObserveError>;

    /// The crossings it predicts we reach, nearest first.
    fn predictions(&self) -> &[Prediction<T>];
}

/// Which way a crossing is going.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Trend {
    /// Nearer than at the fix before.
    Closing,
    /// Neither, by enough to tell over the noise in a fix.
    Holding,
    /// Further than at the fix before, so we are leaving it behind.
    Receding,
}

/// What a predictor can say beyond the prediction itself.
pub trait Trending {
    /// How the distance to `crossing` changed at the last fix. `None` for one the fix before
    /// did not predict, which leaves nothing to compare it against.
    fn trend(&self, crossing: CrossingId) -> Option<Trend>;
}
