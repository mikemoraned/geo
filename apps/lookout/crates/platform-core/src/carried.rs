//! The crossings the device carries.
//!
//! The places a railway meets water, derived from Overture by the water-crossings notebook
//! and packed by `apps/lookout/crates/crossings`.
//!
//! Built into the binary rather than read from a filesystem. The set is a small fraction of
//! the flash either way, so a filesystem would save nothing. It would cost a partition table,
//! a mount at boot, and a way for the device to hold a set the code reading it disagrees with.
//!
//! Regenerate the file with `just carried-crossings`.

use crate::pointset::{Aligned, PointSet, holds_points};

/// The size is written out because `include_bytes!` yields a sized array. A regenerated file
/// of a different size then stops the build, which is when to notice.
static PACKED: &Aligned<[u8; 69_000]> = &Aligned(*include_bytes!("water-crossings.pointset"));

/// A set the reader cannot make sense of stops the build. The bytes are the same on every
/// boot, so a device is the wrong place to find out they are the wrong bytes.
const _: () = assert!(
    holds_points(&PACKED.0),
    "the carried crossings are not a point set this reader understands — repack them",
);

/// The crossings, borrowed from flash.
///
/// Infallible in a binary that compiled. The assertion above asks the reader's own questions
/// of the same bytes, and [`Aligned`] forces the alignment it also checks.
pub fn crossings() -> PointSet<'static> {
    PointSet::new(&PACKED.0).expect("a set checked where it is built into the binary")
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use predictor::{CrowFlies, DEFAULT_RADIUS_METRES, Event, Predict, Sample};

    use super::*;

    /// A set with nothing in it would read, scan, and predict nothing, all without failing.
    #[test]
    fn the_carried_crossings_are_not_empty() {
        assert!(!crossings().is_empty());
    }

    /// The check that matters for the alignment wrapper: without it this is where the cast
    /// would fail, rather than somewhere on the device.
    #[test]
    fn the_carried_bytes_are_aligned_for_casting() {
        assert_eq!(PACKED.0.as_ptr() as usize % 4, 0);
    }

    #[test]
    fn every_carried_crossing_is_somewhere_in_germany() {
        let crossings = crossings();

        for point in crossings.iter() {
            assert!(
                (47.0..=55.0).contains(&point.latitude) && (6.0..=15.1).contains(&point.longitude),
                "{point:?} is not in Germany",
            );
        }
    }

    /// Dresden Hauptbahnhof, and the five crossings nearest it: the Elbe crossings a train
    /// north out of the station passes. They come from the source GeoParquet, through an
    /// independent haversine in `f64`, so the tests below compare two answers rather than one.
    const DRESDEN_HBF: (f64, f64) = (51.0403, 13.7322);
    const NEAREST_TO_DRESDEN: [(u32, f32); 5] = [
        (0x2620_a981, 2334.9),
        (0x6ad4_b654, 2338.5),
        (0x0ea2_0750, 2343.1),
        (0xe6c6_312b, 2347.3),
        (0x4efe_dc58, 2351.6),
    ];
    /// How far the device's answer may sit from the notebook's. They agree to 0.27 m over
    /// 2.3 km — about what `f32` coordinates cost at this latitude, plus two implementations
    /// rounding the earth differently. A metre covers that and no real disagreement.
    const TOLERANCE_M: f32 = 1.0;

    /// The predictor the device runs, over the carried set, at the station.
    fn at_dresden() -> CrowFlies<f32, PointSet<'static>> {
        let mut predictor = CrowFlies::new(crossings(), DEFAULT_RADIUS_METRES);
        let t = DateTime::<Utc>::from_timestamp_millis(1_785_098_609_000).expect("an instant");
        predictor
            .observe(Event::Sampled(
                Sample::at(t, DRESDEN_HBF.0, DRESDEN_HBF.1).expect("on the globe"),
            ))
            .expect("the first event");
        predictor
    }

    #[test]
    fn the_device_agrees_with_the_notebook_about_what_is_nearest() {
        let predictor = at_dresden();

        let ids: Vec<u32> = predictor
            .predictions()
            .iter()
            .take(NEAREST_TO_DRESDEN.len())
            .map(|prediction| prediction.crossing.value())
            .collect();

        assert_eq!(
            ids,
            NEAREST_TO_DRESDEN.map(|(id, _)| id).to_vec(),
            "a different five crossings, or a different order",
        );
    }

    #[test]
    fn the_device_agrees_with_the_notebook_about_how_far() {
        let predictor = at_dresden();

        for (prediction, (id, metres)) in predictor.predictions().iter().zip(NEAREST_TO_DRESDEN) {
            assert_eq!(prediction.crossing.value(), id);
            assert!(
                (prediction.metres - metres).abs() < TOLERANCE_M,
                "{:08x}: the device says {}m, the notebook says {metres}m",
                id,
                prediction.metres,
            );
        }
    }

    /// Twenty crossings lie within the radius of the station. Membership is the stricter
    /// check: a distance can be slightly out and still rank the same, where a crossing near
    /// the radius falls one side of it or the other.
    #[test]
    fn the_device_agrees_with_the_notebook_about_what_is_near() {
        let predictor = at_dresden();

        assert_eq!(predictor.predictions().len(), 20);
    }
}
