use std::sync::Arc;

use domain::{CrossingCompact, Precision};
use geo::{Distance, Haversine};
use geo_types::Point;
use num_traits::AsPrimitive;
use predictor::Crossings;
use rstar::primitives::GeomWithData;
use rstar::{AABB, RTree};

const SLACK_FACTOR: f64 = 1.01;
const SLACK_METRES: f64 = 10.0;

type Indexed<P> = GeomWithData<[f64; 2], CrossingCompact<P>>;

#[derive(Debug, Clone)]
pub struct IndexedCrossings<P: Precision> {
    tree: Arc<RTree<Indexed<P>>>,
}

impl<P: Precision + AsPrimitive<f64>> IndexedCrossings<P> {
    pub fn new(crossings: &[CrossingCompact<P>]) -> Self {
        let indexed = crossings
            .iter()
            .map(|crossing| GeomWithData::new(lon_lat(crossing.position), *crossing))
            .collect();
        Self {
            tree: Arc::new(RTree::bulk_load(indexed)),
        }
    }
}

impl<P: Precision + AsPrimitive<f64>> Crossings<P> for IndexedCrossings<P> {
    fn all(&self) -> impl Iterator<Item = CrossingCompact<P>> {
        self.tree.iter().map(|indexed| indexed.data)
    }

    fn near(
        &self,
        from: Point<P>,
        radius_metres: P,
    ) -> impl Iterator<Item = (CrossingCompact<P>, P)> {
        self.tree
            .locate_in_envelope(bounds(lon_lat(from), radius_metres.as_()))
            .filter_map(move |indexed| {
                let metres = Haversine.distance(from, indexed.data.position);
                (metres <= radius_metres).then_some((indexed.data, metres))
            })
    }
}

fn lon_lat<P: Precision + AsPrimitive<f64>>(point: Point<P>) -> [f64; 2] {
    [point.x().as_(), point.y().as_()]
}

fn bounds([lon, lat]: [f64; 2], radius_metres: f64) -> AABB<[f64; 2]> {
    let angle = (radius_metres * SLACK_FACTOR + SLACK_METRES) / Haversine.radius();
    let lat_span = angle.to_degrees();
    let south = lat - lat_span;
    let north = lat + lat_span;
    let lon_sine = angle.sin() / lat.to_radians().cos();
    let (west, east) = if south <= -90.0 || north >= 90.0 || lon_sine >= 1.0 {
        (-180.0, 180.0)
    } else {
        let lon_span = lon_sine.asin().to_degrees();
        if lon - lon_span < -180.0 || lon + lon_span > 180.0 {
            (-180.0, 180.0)
        } else {
            (lon - lon_span, lon + lon_span)
        }
    };
    AABB::from_corners([west, south], [east, north])
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use domain::CrossingCompactId;
    use proptest::prelude::*;

    use super::*;

    fn near_by_id<P: Precision>(
        near: impl Iterator<Item = (CrossingCompact<P>, P)>,
    ) -> BTreeMap<CrossingCompactId, P> {
        near.map(|(crossing, metres)| (crossing.id, metres))
            .collect()
    }

    fn crossings_around(
        centre: (f64, f64),
        spread: f64,
    ) -> impl Strategy<Value = Vec<CrossingCompact<f64>>> {
        prop::collection::vec((-spread..spread, -spread..spread), 0..200).prop_map(move |offsets| {
            offsets
                .into_iter()
                .enumerate()
                .filter_map(|(id, (dlat, dlon))| {
                    let lat = (centre.0 + dlat).clamp(-90.0, 90.0);
                    let lon = (centre.1 + dlon + 540.0).rem_euclid(360.0) - 180.0;
                    CrossingCompact::at(id as u32, lat, lon).ok()
                })
                .collect()
        })
    }

    fn centres() -> impl Strategy<Value = (f64, f64)> {
        prop_oneof![
            (47.0..55.0, 6.0..15.0),
            (-89.9..89.9, -179.9..179.9),
            (85.0..89.9, -179.9..179.9),
            (-60.0..60.0, prop_oneof![-179.9..-178.0, 178.0..179.9]),
        ]
    }

    proptest! {
        #[test]
        fn the_index_finds_what_a_scan_finds(
            (centre, crossings) in centres().prop_flat_map(|centre| {
                (Just(centre), crossings_around(centre, 0.5))
            }),
            offset in (-0.2..0.2f64, -0.2..0.2f64),
            radius_metres in 1.0..20_000.0f64,
        ) {
            let from = Point::new(
                (centre.1 + offset.1).clamp(-180.0, 180.0),
                (centre.0 + offset.0).clamp(-90.0, 90.0),
            );

            let scanned = near_by_id(crossings.near(from, radius_metres));
            let indexed = near_by_id(IndexedCrossings::new(&crossings).near(from, radius_metres));

            prop_assert_eq!(indexed, scanned);
        }

        #[test]
        fn the_index_finds_what_a_scan_finds_in_f32(
            crossings in crossings_around((51.0, 10.0), 0.5),
            offset in (-0.2..0.2f32, -0.2..0.2f32),
            radius_metres in 1.0..20_000.0f32,
        ) {
            let crossings: Vec<CrossingCompact<f32>> = crossings
                .iter()
                .map(|crossing| {
                    CrossingCompact::new(
                        crossing.id,
                        Point::new(crossing.position.x() as f32, crossing.position.y() as f32),
                    )
                })
                .collect();
            let from = Point::new(10.0 + offset.1, 51.0 + offset.0);

            let scanned = near_by_id(crossings.near(from, radius_metres));
            let indexed = near_by_id(IndexedCrossings::new(&crossings).near(from, radius_metres));

            prop_assert_eq!(indexed, scanned);
        }
    }

    #[test]
    fn every_crossing_indexed_is_among_all() {
        let crossings: Vec<CrossingCompact<f64>> = (0..5)
            .map(|id| {
                CrossingCompact::at(id, 51.0 + f64::from(id) / 100.0, 10.0).expect("on the globe")
            })
            .collect();

        let all: BTreeMap<CrossingCompactId, Point<f64>> = IndexedCrossings::new(&crossings)
            .all()
            .map(|crossing| (crossing.id, crossing.position))
            .collect();

        assert_eq!(
            all,
            crossings
                .iter()
                .map(|crossing| (crossing.id, crossing.position))
                .collect()
        );
    }
}
