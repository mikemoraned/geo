//! The crossings the device carries.
//!
//! **The real ones**: 5,749 places a railway meets water in Germany, derived from Overture by
//! the water-crossings notebook and packed by `apps/lookout/crates/crossings`. Spike 5 carried
//! made-up points of the same size to find out what a scan costs; this one carries the set the
//! predictor will actually be asked about.
//!
//! Built into the binary rather than read from a filesystem. At 69 KB against 8 MB of flash
//! the saving from a filesystem would be nothing, and it would cost a partition table, a mount
//! at boot, and a way for the device to hold a set that disagrees with the code reading it.
//! Regenerate with:
//!
//! ```sh
//! cargo run -p crossings --bin pack_crossings -- --input <crossing_reps.parquet> \
//!     --output spikes/m5/spike6-crossings/core/src/water-crossings.pointset
//! ```

use crate::pointset::{FormatError, PointSet};

/// `include_bytes!` yields a buffer aligned to 1, and the columns are cast in place — so the
/// alignment has to be forced here or the cast fails (and, unchecked, would fault on Xtensa).
#[repr(C, align(4))]
struct Aligned<T: ?Sized>(T);

/// The size is written out because `include_bytes!` yields a sized array: if the file is
/// regenerated at a different size, this stops compiling, which is the right moment to notice.
static PACKED: &Aligned<[u8; 69_000]> = &Aligned(*include_bytes!("water-crossings.pointset"));

/// The crossings, borrowed from flash.
pub fn crossings() -> Result<PointSet<'static>, FormatError> {
    PointSet::new(&PACKED.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As many as the real German crossings set holds.
    const EXPECTED: usize = 5_749;

    #[test]
    fn the_carried_crossings_load() {
        let crossings = crossings().expect("the built-in set");

        assert_eq!(crossings.len(), EXPECTED);
    }

    /// The check that matters for the alignment wrapper: without it this is where the cast
    /// would fail, rather than somewhere on the device.
    #[test]
    fn the_carried_bytes_are_aligned_for_casting() {
        assert_eq!(PACKED.0.as_ptr() as usize % 4, 0);
    }

    #[test]
    fn every_carried_crossing_is_somewhere_in_germany() {
        let crossings = crossings().expect("the built-in set");

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
    const DRESDEN_HBF: (f32, f32) = (51.0403, 13.7322);
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

    #[test]
    fn the_device_agrees_with_the_notebook_about_what_is_nearest() {
        let crossings = crossings().expect("the built-in set");

        let found = crate::scan::nearby(
            &crossings,
            geo::Point::new(DRESDEN_HBF.1, DRESDEN_HBF.0),
            NEAREST_TO_DRESDEN.len(),
            5_000.0,
        );

        let ids: Vec<u32> = found.nearest.iter().map(|near| near.crossing.id).collect();
        assert_eq!(
            ids,
            NEAREST_TO_DRESDEN.map(|(id, _)| id).to_vec(),
            "a different five crossings, or a different order",
        );
    }

    #[test]
    fn the_device_agrees_with_the_notebook_about_how_far() {
        let crossings = crossings().expect("the built-in set");

        let found = crate::scan::nearby(
            &crossings,
            geo::Point::new(DRESDEN_HBF.1, DRESDEN_HBF.0),
            NEAREST_TO_DRESDEN.len(),
            5_000.0,
        );

        for (near, (id, metres)) in found.nearest.iter().zip(NEAREST_TO_DRESDEN) {
            assert_eq!(near.crossing.id, id);
            assert!(
                (near.metres - metres).abs() < TOLERANCE_M,
                "{:08x}: device says {}m, the notebook says {metres}m",
                id,
                near.metres,
            );
        }
    }
    /// The predictor's half of the same scan, against the same source rows: twenty crossings
    /// lie within five kilometres of the station. An `f32` buffer disagreeing with `f64`
    /// source data about a *membership* question is the failure this rules out — a crossing
    /// sitting near the radius could fall the other side of it.
    #[test]
    fn the_device_agrees_with_the_notebook_about_what_is_near() {
        let crossings = crossings().expect("the built-in set");

        let found = crate::scan::nearby(
            &crossings,
            geo::Point::new(DRESDEN_HBF.1, DRESDEN_HBF.0),
            NEAREST_TO_DRESDEN.len(),
            5_000.0,
        );

        assert_eq!(found.within.len(), 20);
    }
}
