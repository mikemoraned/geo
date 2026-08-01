//! The crossings the device carries.
//!
//! Built into the binary rather than read from a filesystem: 69 KB is small enough to sit in
//! flash, and it leaves nothing to go wrong between power-on and the first fix.
//!
//! These are **made-up** crossings — 5,749 of them, as many as the real German set holds,
//! scattered over the same extent. What this spike measures is what a scan of that many points
//! costs, and made-up points cost exactly what real ones do. Regenerate with:
//!
//! ```sh
//! cargo run -p crossings --bin random_crossings -- \
//!     --output spikes/m5/spike5-distance/core/src/random-crossings.pointset
//! ```

use crate::pointset::{FormatError, PointSet};

/// `include_bytes!` yields a buffer aligned to 1, and the columns are cast in place — so the
/// alignment has to be forced here or the cast fails (and, unchecked, would fault on Xtensa).
#[repr(C, align(4))]
struct Aligned<T: ?Sized>(T);

/// The size is written out because `include_bytes!` yields a sized array: if the file is
/// regenerated at a different size, this stops compiling, which is the right moment to notice.
static PACKED: &Aligned<[u8; 69_000]> = &Aligned(*include_bytes!("random-crossings.pointset"));

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
}
