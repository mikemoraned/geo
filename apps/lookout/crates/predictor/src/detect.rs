use std::collections::BTreeSet;

use domain::{CrossingCompactId, Precision};

use crate::predict::{Event, ObserveError};

pub type Passes = BTreeSet<CrossingCompactId>;

pub trait DetectPasses<P: Precision> {
    fn observe(&mut self, event: Event<P>) -> Result<(), ObserveError>;

    fn passed(&self) -> &Passes;
}
