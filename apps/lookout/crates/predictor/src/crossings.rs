//! Where a predictor reads the crossings it scans.

use domain::{CrossingCompact, Measure};

/// A source rather than a slice, because the platforms hold a set differently. The device
/// keeps thousands of crossings in flash, as three parallel columns, and scans them where they
/// lie. Copying those into a `Vec` would cost tens of kilobytes of RAM the board has better
/// uses for. Off the device a `Vec` is the obvious source, and is one.
pub trait Crossings<T: Measure> {
    /// Every crossing in the set, in whatever order it is held in. A scan reads all of them,
    /// so the order is the source's to choose.
    ///
    /// Named `all` rather than `iter` because every source that implements this has an
    /// inherent `iter` of its own, yielding something else.
    fn all(&self) -> impl Iterator<Item = CrossingCompact<T>>;
}

impl<T: Measure> Crossings<T> for Vec<CrossingCompact<T>> {
    fn all(&self) -> impl Iterator<Item = CrossingCompact<T>> {
        self.iter().copied()
    }
}
