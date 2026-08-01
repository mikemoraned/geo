//! Reading the packed crossings buffer, in place.
//!
//! The device holds the whole set in flash and scans it against every fix, so nothing here
//! copies or allocates: the columns are the file's own bytes, cast where they lie.
//!
//! **This is the second implementation of the format.** The packer that writes it is
//! `apps/lookout/crates/crossings`, whose README is the layout's definition; the constants
//! below are repeated here rather than shared, because that crate reads GeoParquet through
//! arrow and could never be built for this target — and because a spike has to keep building
//! years later, which a path dependency on a moving crate would not survive.
//! `tests/four-crossings.pointset` is a real file from that packer, and the test that reads it
//! is what makes the repetition safe.

use bytemuck::PodCastError;

/// Names the format in the first bytes of the file.
const MAGIC: [u8; 4] = *b"XING";
/// The layout this reader understands.
const VERSION: u32 = 1;
/// Magic, version, count. A multiple of 4, so the columns after it are aligned.
const HEADER_LEN: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
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
    /// `include_bytes!` yields a buffer aligned to 1, so this is what happens without a
    /// `#[repr(align(4))]` wrapper around it — a fault on Xtensa if it were not caught.
    #[error("not aligned for 4-byte columns")]
    Misaligned,
}

/// One crossing, as the buffer holds it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub id: u32,
    pub latitude: f32,
    pub longitude: f32,
}

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
        let version = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        if version != VERSION {
            return Err(FormatError::UnsupportedVersion { found: version });
        }

        let count = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
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

        Ok(Self {
            latitudes: floats(column(0))?,
            longitudes: floats(column(1))?,
            ids: words(column(2))?,
        })
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// The columns, for a scan that wants to read position without touching the ids.
    pub fn positions(&self) -> impl Iterator<Item = (f32, f32)> + '_ {
        self.latitudes
            .iter()
            .copied()
            .zip(self.longitudes.iter().copied())
    }

    pub fn get(&self, index: usize) -> Option<Point> {
        Some(Point {
            id: *self.ids.get(index)?,
            latitude: self.latitudes[index],
            longitude: self.longitudes[index],
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = Point> + '_ {
        (0..self.len()).filter_map(|index| self.get(index))
    }
}

/// Casting rather than copying is the point: the columns stay in flash, where a 5,749-point
/// set has no business being duplicated into RAM.
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
    #[repr(C, align(4))]
    struct Aligned<T: ?Sized>(T);
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
    fn positions_and_points_describe_the_same_crossings() {
        let points = PointSet::new(four_crossings()).unwrap();

        let from_columns: Vec<_> = points.positions().collect();
        let from_points: Vec<_> = points
            .iter()
            .map(|point| (point.latitude, point.longitude))
            .collect();

        assert_eq!(from_columns, from_points);
    }

    #[test]
    fn an_index_past_the_end_is_none() {
        let points = PointSet::new(four_crossings()).unwrap();

        assert!(points.get(3).is_some());
        assert!(points.get(4).is_none());
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
