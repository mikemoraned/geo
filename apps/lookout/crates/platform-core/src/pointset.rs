//! Reading the packed crossings buffer, in place.
//!
//! The device holds the whole set in flash and scans it against every fix, so nothing here
//! copies or allocates: the columns are the file's own bytes, cast where they lie.
//!
//! This is the second implementation of the format. `apps/lookout/crates/crossings` packs the
//! file and its README defines the layout, but that crate reads GeoParquet through arrow and
//! could never build for this target, so the constants below are repeated rather than shared.
//! `tests/four-crossings.pointset` is a real file from the packer, and the test that reads it
//! is what makes the repetition safe.

use bytemuck::PodCastError;
use predictor::{Crossing, CrossingId, Crossings};

/// Names the format in the first bytes of the file.
const MAGIC: [u8; 4] = *b"XING";
/// The layout this reader understands.
const VERSION: u32 = 1;
/// Magic, version, count. A multiple of 4, so the columns after it are aligned.
const HEADER_LEN: usize = 12;

/// Not `Eq`: a coordinate is a float, and the error carries the one that was wrong.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum FormatError {
    #[error("{0} bytes is too short to hold a header")]
    NoHeader(usize),
    #[error("does not start with XING")]
    NotAPointSet,
    #[error("version {found}, which this reader does not know")]
    UnsupportedVersion { found: u32 },
    #[error("claims {points} points, which needs {needed} bytes, but there are {got}")]
    Truncated {
        points: u32,
        needed: usize,
        got: usize,
    },
    /// What an unwrapped `include_bytes!` buffer gives, since it is aligned to 1. Uncaught,
    /// it would fault on Xtensa.
    #[error("not aligned for 4-byte columns")]
    Misaligned,
    /// Found once here rather than per scan. Unchecked, such a coordinate would be measured
    /// against every fix for the rest of the run.
    #[error("holds {latitude},{longitude}, which is not on the globe")]
    OffTheGlobe { latitude: f32, longitude: f32 },
}

/// One crossing, as the buffer holds it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub id: u32,
    pub latitude: f32,
    pub longitude: f32,
}

/// Forces the 4-byte alignment the columns are cast at. Anything holding packed bytes goes
/// behind it, because `include_bytes!` yields a buffer aligned to 1.
#[repr(C, align(4))]
pub(crate) struct Aligned<T: ?Sized>(pub T);

/// The crossings, borrowed from the bytes they are stored in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointSet<'a> {
    latitudes: &'a [f32],
    longitudes: &'a [f32],
    ids: &'a [u32],
}

impl<'a> PointSet<'a> {
    /// Borrows the points from a packed buffer, checking that it is one.
    pub fn new(packed: &'a [u8]) -> Result<Self, FormatError> {
        let header = packed
            .get(..HEADER_LEN)
            .ok_or(FormatError::NoHeader(packed.len()))?;

        if header[..4] != MAGIC {
            return Err(FormatError::NotAPointSet);
        }
        let version = word(header, 4);
        if version != VERSION {
            return Err(FormatError::UnsupportedVersion { found: version });
        }

        let count = word(header, 8);
        let points = count as usize;
        let column_len = points * 4;
        let needed = HEADER_LEN + column_len * 3;
        if packed.len() != needed {
            return Err(FormatError::Truncated {
                points: count,
                needed,
                got: packed.len(),
            });
        }

        let column = |index: usize| &packed[HEADER_LEN + index * column_len..][..column_len];

        let set = Self {
            latitudes: floats(column(0))?,
            longitudes: floats(column(1))?,
            ids: words(column(2))?,
        };
        set.on_the_globe()?;
        Ok(set)
    }

    /// Checks every coordinate once, so a scan never has to.
    ///
    /// The columns are bytes, and bytes arrive corrupt. Swept here, a coordinate the earth has
    /// no room for is a fact about the buffer; found later, it is a distance the predictor
    /// reports as a prediction.
    fn on_the_globe(&self) -> Result<(), FormatError> {
        self.positions()
            .find(|(latitude, longitude)| {
                !(-90.0..=90.0).contains(latitude) || !(-180.0..=180.0).contains(longitude)
            })
            .map_or(Ok(()), |(latitude, longitude)| {
                Err(FormatError::OffTheGlobe {
                    latitude,
                    longitude,
                })
            })
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The position columns, without touching the ids.
    fn positions(&self) -> impl Iterator<Item = (f32, f32)> + '_ {
        self.latitudes
            .iter()
            .copied()
            .zip(self.longitudes.iter().copied())
    }

    pub fn iter(&self) -> impl Iterator<Item = Point> + '_ {
        self.positions()
            .zip(self.ids.iter().copied())
            .map(|((latitude, longitude), id)| Point {
                id,
                latitude,
                longitude,
            })
    }
}

/// Whether a buffer holds a point set this reader understands, decided at compile time.
///
/// It asks the three questions [`PointSet::new`] asks — the magic, the version, and whether
/// the length matches the count claimed — of bytes built into the binary, so a set the reader
/// cannot make sense of stops the build rather than reaching a device. The answer is a `bool`
/// because that is what a `const` assertion acts on. `new` is where a failure is described.
pub(crate) const fn holds_points(packed: &[u8]) -> bool {
    if packed.len() < HEADER_LEN {
        return false;
    }

    let mut index = 0;
    while index < MAGIC.len() {
        if packed[index] != MAGIC[index] {
            return false;
        }
        index += 1;
    }

    if word(packed, 4) != VERSION {
        return false;
    }
    packed.len() == HEADER_LEN + word(packed, 8) as usize * 4 * 3
}

/// One little-endian `u32` at `at`, in a `const` context, where `from_le_bytes` over a slice
/// is not available.
const fn word(packed: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([packed[at], packed[at + 1], packed[at + 2], packed[at + 3]])
}

/// A set is what the predictor scans, read straight out of flash.
///
/// Nothing here converts or checks. The coordinates were checked when the buffer was read, and
/// they already hold the float the scan measures in — which matters on a board that emulates
/// `f64` in software and scans the whole set every second.
impl Crossings<f32> for PointSet<'_> {
    fn all(&self) -> impl Iterator<Item = Crossing<f32>> {
        PointSet::iter(self).map(|point| {
            Crossing::new(
                CrossingId::new(point.id),
                geo_types::Point::new(point.longitude, point.latitude),
            )
        })
    }
}

/// Casting rather than copying is the point: the columns stay in flash, where the set has no
/// business being duplicated into RAM.
fn floats(column: &[u8]) -> Result<&[f32], FormatError> {
    bytemuck::try_cast_slice(column).map_err(misalignment)
}

fn words(column: &[u8]) -> Result<&[u32], FormatError> {
    bytemuck::try_cast_slice(column).map_err(misalignment)
}

/// A length that isn't a multiple of 4 is already ruled out by the header check, so what is
/// left is an address that isn't.
fn misalignment(_: PodCastError) -> FormatError {
    FormatError::Misaligned
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real file from the packer, four crossings around Ruhland and Ortrand. It is what
    /// keeps this reader honest about a layout it repeats rather than imports: if the two
    /// drift apart, this stops reading.
    static FOUR_CROSSINGS: &Aligned<[u8; 60]> =
        &Aligned(*include_bytes!("../tests/four-crossings.pointset"));

    /// Those four crossings' positions, as the dataset's geometry holds them, narrowed to the
    /// `f32` the buffer stores. Each is the exact stored value — a rounded copy would still
    /// pass against a reader that was itself slightly wrong.
    const EXPECTED: [(f32, f32); 4] = [
        (51.61757, 13.548209),
        (51.665596, 13.584108),
        (51.66559, 13.58417),
        (51.617466, 13.548138),
    ];

    fn four_crossings() -> &'static [u8] {
        &FOUR_CROSSINGS.0
    }

    fn packed(points: &[Point]) -> Vec<u8> {
        let mut packed = Vec::new();
        packed.extend_from_slice(&MAGIC);
        packed.extend_from_slice(&VERSION.to_le_bytes());
        packed.extend_from_slice(&(points.len() as u32).to_le_bytes());
        packed.extend(points.iter().flat_map(|point| point.latitude.to_le_bytes()));
        packed.extend(
            points
                .iter()
                .flat_map(|point| point.longitude.to_le_bytes()),
        );
        packed.extend(points.iter().flat_map(|point| point.id.to_le_bytes()));
        packed
    }

    /// Aligns a buffer built at run time, as `#[repr(align(4))]` does for a static one.
    fn aligned(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks(4)
            .map(|word| {
                let mut whole = [0u8; 4];
                whole[..word.len()].copy_from_slice(word);
                u32::from_le_bytes(whole)
            })
            .collect()
    }

    fn read(words: &[u32], len: usize) -> Result<PointSet<'_>, FormatError> {
        PointSet::new(&bytemuck::cast_slice(words)[..len])
    }

    #[test]
    fn a_file_from_the_packer_reads_back() {
        let points = PointSet::new(four_crossings()).unwrap();

        assert_eq!(points.len(), 4);
        for (latitude, longitude) in EXPECTED {
            assert!(
                points
                    .iter()
                    .any(|point| point.latitude == latitude && point.longitude == longitude),
                "{latitude},{longitude} is not in the set",
            );
        }
    }

    #[test]
    fn every_crossing_in_a_file_has_its_own_id() {
        let points = PointSet::new(four_crossings()).unwrap();

        let mut ids: Vec<u32> = points.iter().map(|point| point.id).collect();
        ids.sort_unstable();
        ids.dedup();

        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn a_set_yields_every_crossing_it_holds() {
        let points = PointSet::new(four_crossings()).unwrap();

        assert_eq!(points.iter().count(), points.len());
    }

    /// The columns are read once when the buffer is, so a corrupt one is a fact about the
    /// file rather than a distance the predictor reports every second for the rest of the run.
    #[test]
    fn a_coordinate_off_the_globe_is_refused() {
        let bytes = packed(&[Point {
            id: 1,
            latitude: 91.0,
            longitude: 13.5,
        }]);
        let words = aligned(&bytes);

        assert!(matches!(
            read(&words, bytes.len()),
            Err(FormatError::OffTheGlobe { .. })
        ));
    }

    #[test]
    fn an_empty_set_is_readable_and_empty() {
        let bytes = packed(&[]);
        let words = aligned(&bytes);

        let points = read(&words, bytes.len()).unwrap();

        assert!(points.is_empty());
        assert_eq!(points.iter().count(), 0);
    }

    /// The trap `include_bytes!` sets: it yields a buffer aligned to 1, and casting that to
    /// `&[f32]` faults on Xtensa. Caught here rather than on the device.
    #[test]
    fn a_misaligned_buffer_is_refused_rather_than_faulting() {
        let bytes = packed(&[Point {
            id: 1,
            latitude: 51.6,
            longitude: 13.5,
        }]);
        // One byte of padding in front, so the columns land on an odd address.
        let mut shifted = vec![0u8];
        shifted.extend_from_slice(&bytes);
        let words = aligned(&shifted);
        let misaligned = &bytemuck::cast_slice::<u32, u8>(&words)[1..][..bytes.len()];

        assert_eq!(PointSet::new(misaligned), Err(FormatError::Misaligned));
    }

    #[test]
    fn something_that_is_not_a_point_set_is_refused() {
        let mut bytes = packed(&[]);
        bytes[..4].copy_from_slice(b"PARQ");
        let words = aligned(&bytes);

        assert_eq!(read(&words, bytes.len()), Err(FormatError::NotAPointSet));
    }

    #[test]
    fn a_version_this_reader_does_not_know_is_refused() {
        let mut bytes = packed(&[]);
        bytes[4..8].copy_from_slice(&(VERSION + 1).to_le_bytes());
        let words = aligned(&bytes);

        assert_eq!(
            read(&words, bytes.len()),
            Err(FormatError::UnsupportedVersion { found: VERSION + 1 })
        );
    }

    #[test]
    fn a_buffer_too_short_for_a_header_is_refused() {
        let words = aligned(&MAGIC);

        assert_eq!(read(&words, 4), Err(FormatError::NoHeader(4)));
    }

    /// Without this the reader would slice past the end of the buffer and panic, or read
    /// points made of whatever followed it in flash.
    #[test]
    fn a_buffer_that_does_not_hold_the_points_it_claims_is_refused() {
        let bytes = packed(&[Point {
            id: 1,
            latitude: 51.6,
            longitude: 13.5,
        }]);
        let words = aligned(&bytes);

        assert!(matches!(
            read(&words, bytes.len() - 4),
            Err(FormatError::Truncated { points: 1, .. })
        ));
    }
}
