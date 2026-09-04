//! The crossings the device carries.
//!
//! The places a railway meets water, derived from Overture by the water-crossings notebook
//! and packed by `apps/lookout/crates/crossings`.
//!
//! Built into the binary rather than read from a filesystem. The set is a small fraction of
//! the flash it would sit in either way, so a filesystem would save nothing, and it would
//! cost a partition table, a mount at boot, and a way for the device to hold a set that
//! disagrees with the code reading it. Regenerate the file with `just carried-crossings`.

use crate::pointset::{PointSet, holds_points};

/// `include_bytes!` yields a buffer aligned to 1, and the columns are cast in place — so the
/// alignment has to be forced here or the cast fails (and, unchecked, would fault on Xtensa).
#[repr(C, align(4))]
struct Aligned<T: ?Sized>(T);

/// The size is written out because `include_bytes!` yields a sized array: if the file is
/// regenerated at a different size, this stops compiling, which is the right moment to notice.
static PACKED: &Aligned<[u8; 69_000]> = &Aligned(*include_bytes!("water-crossings.pointset"));

/// A set the reader cannot make sense of stops the build. The bytes are the same on every
/// boot, so a device is the wrong place to discover they are the wrong bytes, and a panel
/// reporting it is a shipped binary that can never predict anything.
const _: () = assert!(
    holds_points(&PACKED.0),
    "the carried crossings are not a point set this reader understands — repack them",
);

/// The crossings, borrowed from flash.
///
/// Infallible in a binary that compiled. The assertion above asks the same questions of the
/// same bytes as the reader does, and the alignment the reader also checks is forced by
/// [`Aligned`].
pub fn crossings() -> PointSet<'static> {
    PointSet::new(&PACKED.0).expect("a set checked where it is built into the binary")
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use predictor::{CrowFlies, Event, Predict, Sample};

    use super::*;
    use crate::WITHIN_METRES;

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

    /// Dresden Hauptbahnhof, and the five crossings nearest it — worked out from the source
    /// GeoParquet with an independent haversine, in `f64`, before any of this ran. They are
    /// the same handful of Elbe crossings a train north out of the station passes.
    ///
    /// This is the end-to-end check that matters for the real set: the notebook's answer and
    /// the device's answer, over the same source rows, have to be the same answer.
    const DRESDEN_HBF: (f64, f64) = (51.0403, 13.7322);
    const NEAREST_TO_DRESDEN: [(u32, f32); 5] = [
        (0x2620_a981, 2334.9),
        (0x6ad4_b654, 2338.5),
        (0x0ea2_0750, 2343.1),
        (0xe6c6_312b, 2347.3),
        (0x4efe_dc58, 2351.6),
    ];
    /// How far the device's answer may sit from the notebook's. They actually agree to
    /// **0.27 m over 2.3 km** — about what the `f32` coordinates cost (~0.2 m at this
    /// latitude) plus the two implementations using slightly different mean earth radii.
    /// A metre leaves room for that without leaving room for a real disagreement.
    const TOLERANCE_M: f32 = 1.0;

    /// The predictor the device runs, over the carried set, at the station.
    fn at_dresden() -> CrowFlies<f32, PointSet<'static>> {
        let mut predictor = CrowFlies::new(crossings(), WITHIN_METRES);
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

    /// The predictor's radius over the same source rows: twenty crossings lie within five
    /// kilometres of the station. An `f32` buffer disagreeing with `f64` source data about a
    /// *membership* question is the failure this rules out — a crossing sitting near the
    /// radius could fall the other side of it.
    #[test]
    fn the_device_agrees_with_the_notebook_about_what_is_near() {
        let predictor = at_dresden();

        assert_eq!(predictor.predictions().len(), 20);
    }
}
