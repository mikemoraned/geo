use bytemuck::PodCastError;
use domain::CrossingCompact;
use predictor::Crossings;

const MAGIC: [u8; 4] = *b"XING";
const VERSION: u32 = 1;
const HEADER_LEN: usize = 12;

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
    #[error("not aligned for 4-byte columns")]
    Misaligned,
    #[error("holds {latitude},{longitude}, which is not on the globe")]
    OffTheGlobe { latitude: f32, longitude: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub id: u32,
    pub latitude: f32,
    pub longitude: f32,
}

#[repr(C, align(4))]
pub struct Aligned<T: ?Sized>(pub T);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointSet<'a> {
    latitudes: &'a [f32],
    longitudes: &'a [f32],
    ids: &'a [u32],
}

impl PointSet<'static> {
    pub fn empty() -> Self {
        static EMPTY: &Aligned<[u8; HEADER_LEN]> = &Aligned([
            MAGIC[0], MAGIC[1], MAGIC[2], MAGIC[3], 1, 0, 0, 0, 0, 0, 0, 0,
        ]);

        Self::new(&EMPTY.0).expect("a header this module wrote")
    }
}

impl Default for PointSet<'static> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<'a> PointSet<'a> {
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

pub const fn holds_points(packed: &[u8]) -> bool {
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

const fn word(packed: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([packed[at], packed[at + 1], packed[at + 2], packed[at + 3]])
}

impl Crossings<f32> for PointSet<'_> {
    fn all(&self) -> impl Iterator<Item = CrossingCompact<f32>> {
        PointSet::iter(self).map(|point| {
            CrossingCompact::new(
                point.id,
                geo_types::Point::new(point.longitude, point.latitude),
            )
        })
    }
}

fn floats(column: &[u8]) -> Result<&[f32], FormatError> {
    bytemuck::try_cast_slice(column).map_err(misalignment)
}

fn words(column: &[u8]) -> Result<&[u32], FormatError> {
    bytemuck::try_cast_slice(column).map_err(misalignment)
}

fn misalignment(_: PodCastError) -> FormatError {
    FormatError::Misaligned
}

#[cfg(test)]
mod tests {
    use super::*;

    static FOUR_CROSSINGS: &Aligned<[u8; 60]> =
        &Aligned(*include_bytes!("../tests/four-crossings.pointset"));

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
    fn the_empty_set_holds_nothing() {
        let points = PointSet::empty();

        assert!(points.is_empty());
        assert_eq!(points.iter().count(), 0);
    }

    #[test]
    fn an_empty_set_is_readable_and_empty() {
        let bytes = packed(&[]);
        let words = aligned(&bytes);

        let points = read(&words, bytes.len()).unwrap();

        assert!(points.is_empty());
        assert_eq!(points.iter().count(), 0);
    }

    const ONE_BYTE_OF_PADDING: usize = 1;

    #[test]
    fn a_misaligned_buffer_is_refused_rather_than_faulting() {
        let bytes = packed(&[Point {
            id: 1,
            latitude: 51.6,
            longitude: 13.5,
        }]);
        let mut shifted = vec![0u8; ONE_BYTE_OF_PADDING];
        shifted.extend_from_slice(&bytes);
        let words = aligned(&shifted);
        let misaligned =
            &bytemuck::cast_slice::<u32, u8>(&words)[ONE_BYTE_OF_PADDING..][..bytes.len()];

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
