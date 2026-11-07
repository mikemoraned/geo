use domain::{CrossingCompact, Precision};
use geo::{Distance, Haversine};
use geo_types::Point;

pub trait Crossings<P: Precision> {
    fn all(&self) -> impl Iterator<Item = CrossingCompact<P>>;

    fn near(
        &self,
        from: Point<P>,
        radius_metres: P,
    ) -> impl Iterator<Item = (CrossingCompact<P>, P)> {
        self.all().filter_map(move |crossing| {
            let metres = Haversine.distance(from, crossing.position);
            (metres <= radius_metres).then_some((crossing, metres))
        })
    }
}

impl<P: Precision> Crossings<P> for Vec<CrossingCompact<P>> {
    fn all(&self) -> impl Iterator<Item = CrossingCompact<P>> {
        self.iter().copied()
    }
}
