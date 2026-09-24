use domain::{CrossingCompact, Precision};

pub trait Crossings<P: Precision> {
    fn all(&self) -> impl Iterator<Item = CrossingCompact<P>>;
}

impl<P: Precision> Crossings<P> for Vec<CrossingCompact<P>> {
    fn all(&self) -> impl Iterator<Item = CrossingCompact<P>> {
        self.iter().copied()
    }
}
