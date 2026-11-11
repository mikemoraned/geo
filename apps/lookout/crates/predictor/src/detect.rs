use std::collections::BTreeSet;

use domain::{CrossingCompactId, Precision};

use crate::clock::OutOfOrder;
use crate::predict::Event;

pub type Passes = BTreeSet<CrossingCompactId>;

pub trait DetectPasses<P: Precision> {
    fn observe(&mut self, event: Event<P>) -> Result<(), OutOfOrder>;

    fn passing(&self) -> &Passes;
}
