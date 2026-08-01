//! The packed point buffer the device scans.
//!
//! Laid out for a brute-force scan on a 240 MHz microcontroller with no filesystem worth the
//! name: a short header, then three parallel columns the device casts in place rather than
//! parsing. See `README.md` for the byte layout and what a reader must check.
//!
//! Coordinates are `f32` degrees. Over the German crossings that costs at most 0.21 m of
//! position, far under what GPS resolves, and it is what the ESP32's single-precision FPU
//! wants: `f64` there is emulated in software.

use std::fmt::{self, Display};

use geo_types::Coord;

use crate::id::CrossingId;
use crate::silver::Crossing;

/// Names the format in the first bytes of the file, so a reader handed the wrong file says so
/// instead of reading coordinates out of it.
pub const MAGIC: [u8; 4] = *b"XING";
/// Bumped whenever the layout changes in a way an existing reader would misread.
pub const VERSION: u32 = 1;
/// Magic, version, count — 12 bytes, which is itself a multiple of 4, so the columns after it
/// are aligned without padding.
pub const HEADER_LEN: usize = 12;
/// One `f32` latitude, one `f32` longitude, one `u32` id.
pub const BYTES_PER_POINT: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FormatError {
    #[error("{0} bytes is too short to hold a {HEADER_LEN}-byte header")]
    NoHeader(usize),
    #[error("does not start with {:?}", String::from_utf8_lossy(&MAGIC))]
    NotAPointSet,
    #[error("version {found}, which this reader does not know (it reads {VERSION})")]
    UnsupportedVersion { found: u32 },
    #[error("header claims {points} points, which needs {needed} bytes, but there are {got}")]
    Truncated {
        points: u32,
        needed: usize,
        got: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0} points is more than a u32 count can name")]
pub struct TooManyPoints(usize);

/// One crossing as the device holds it: where it is, and what it is called.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub id: CrossingId,
    pub latitude: f32,
    pub longitude: f32,
}

impl Point {
    pub fn new(id: CrossingId, position: Coord<f64>) -> Self {
        Self {
            id,
            latitude: position.y as f32,
            longitude: position.x as f32,
        }
    }

    pub fn of(crossing: &Crossing, id: CrossingId) -> Self {
        Self::new(id, crossing.position)
    }
}

impl Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {},{}", self.id, self.latitude, self.longitude)
    }
}

/// The packed bytes for these points.
///
/// Points are written in id order, so the same crossings pack to the same bytes however the
/// dataset that produced them happened to be ordered.
pub fn pack(points: &[Point]) -> Result<Vec<u8>, TooManyPoints> {
    let count = u32::try_from(points.len()).map_err(|_| TooManyPoints(points.len()))?;

    let mut ordered: Vec<&Point> = points.iter().collect();
    ordered.sort_by_key(|point| point.id);

    let mut packed = Vec::with_capacity(HEADER_LEN + points.len() * BYTES_PER_POINT);
    packed.extend_from_slice(&MAGIC);
    packed.extend_from_slice(&VERSION.to_le_bytes());
    packed.extend_from_slice(&count.to_le_bytes());

    packed.extend(
        ordered
            .iter()
            .flat_map(|point| point.latitude.to_le_bytes()),
    );
    packed.extend(
        ordered
            .iter()
            .flat_map(|point| point.longitude.to_le_bytes()),
    );
    packed.extend(
        ordered
            .iter()
            .flat_map(|point| point.id.get().to_le_bytes()),
    );

    Ok(packed)
}

/// The points a packed buffer holds.
///
/// The device reads the same bytes by casting them in place; this reads them field by field
/// so that a round-trip here checks the layout rather than the host's memory representation.
pub fn unpack(packed: &[u8]) -> Result<Vec<Point>, FormatError> {
    let header = packed
        .get(..HEADER_LEN)
        .ok_or(FormatError::NoHeader(packed.len()))?;

    if header[..4] != MAGIC {
        return Err(FormatError::NotAPointSet);
    }
    let version = u32::from_le_bytes(header[4..8].try_into().expect("4 bytes of header"));
    if version != VERSION {
        return Err(FormatError::UnsupportedVersion { found: version });
    }

    let count = u32::from_le_bytes(header[8..12].try_into().expect("4 bytes of header"));
    let points = count as usize;
    let needed = HEADER_LEN + points * BYTES_PER_POINT;
    if packed.len() != needed {
        return Err(FormatError::Truncated {
            points: count,
            needed,
            got: packed.len(),
        });
    }

    let column = |index: usize| &packed[HEADER_LEN + index * points * 4..][..points * 4];
    let (latitudes, longitudes, ids) = (column(0), column(1), column(2));
    let word = |column: &[u8], row: usize| {
        <[u8; 4]>::try_from(&column[row * 4..][..4]).expect("a column holds 4 bytes per point")
    };

    Ok((0..points)
        .map(|row| Point {
            latitude: f32::from_le_bytes(word(latitudes, row)),
            longitude: f32::from_le_bytes(word(longitudes, row)),
            id: CrossingId::from_bits(u32::from_le_bytes(word(ids, row))),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use geo_types::coord;

    use super::*;
    use crate::id::Key;

    /// A crossing near Ruhland, and one near Dresden.
    const RUHLAND: (f64, f64) = (13.548209, 51.617567);
    const DRESDEN: (f64, f64) = (13.733, 51.05);

    fn point(name: &str, (lon, lat): (f64, f64)) -> Point {
        Point::new(
            CrossingId::of(&Key::new(name, "water", 0.5)),
            coord! { x: lon, y: lat },
        )
    }

    fn points() -> Vec<Point> {
        vec![point("ruhland", RUHLAND), point("dresden", DRESDEN)]
    }

    /// The property the device depends on: what the packer wrote is what a reader of the
    /// documented layout gets back.
    #[test]
    fn points_survive_a_round_trip() {
        let points = points();

        let read = unpack(&pack(&points).unwrap()).unwrap();

        assert_eq!(read.len(), points.len());
        for point in &points {
            assert!(read.contains(point), "{point} did not come back");
        }
    }

    /// f32 degrees are the whole reason the buffer is this small, so the loss they cost is
    /// worth stating: under a metre, which is under what the receiver resolves.
    #[test]
    fn a_position_survives_to_within_a_metre() {
        const METRES_PER_DEGREE: f64 = 111_320.0;

        let read = unpack(&pack(&points()).unwrap()).unwrap();
        let ruhland = read
            .iter()
            .find(|read| read.id == points()[0].id)
            .expect("the crossing that was packed");

        let (lon, lat) = RUHLAND;
        let north = (f64::from(ruhland.latitude) - lat).abs() * METRES_PER_DEGREE;
        let east =
            (f64::from(ruhland.longitude) - lon).abs() * METRES_PER_DEGREE * lat.to_radians().cos();
        assert!(north < 1.0 && east < 1.0, "{north}m north, {east}m east");
    }

    #[test]
    fn an_empty_set_packs_to_a_header_and_reads_back_empty() {
        let packed = pack(&[]).unwrap();

        assert_eq!(packed.len(), HEADER_LEN);
        assert_eq!(unpack(&packed).unwrap(), vec![]);
    }

    #[test]
    fn a_buffer_is_a_header_plus_twelve_bytes_a_point() {
        assert_eq!(
            pack(&points()).unwrap().len(),
            HEADER_LEN + 2 * BYTES_PER_POINT
        );
    }

    /// The device casts the columns in place, which is only sound if each starts on a 4-byte
    /// boundary — so the header's length has to stay a multiple of 4.
    #[test]
    fn every_column_starts_four_byte_aligned() {
        let packed = pack(&points()).unwrap();

        assert_eq!(HEADER_LEN % 4, 0);
        assert_eq!((packed.len() - HEADER_LEN) % 4, 0);
    }

    /// So that the same crossings pack to the same bytes whatever order they arrive in, and a
    /// rebuild that reorders rows doesn't reflash the device with an identical dataset.
    #[test]
    fn the_bytes_do_not_depend_on_the_order_the_points_arrive_in() {
        let mut reversed = points();
        reversed.reverse();

        assert_eq!(pack(&points()).unwrap(), pack(&reversed).unwrap());
    }

    #[test]
    fn a_buffer_starts_with_the_magic_and_the_version() {
        let packed = pack(&points()).unwrap();

        assert_eq!(&packed[..4], &MAGIC);
        assert_eq!(&packed[4..8], &VERSION.to_le_bytes());
        assert_eq!(&packed[8..12], &2u32.to_le_bytes());
    }

    #[test]
    fn something_that_is_not_a_point_set_is_rejected() {
        let mut packed = pack(&points()).unwrap();
        packed[..4].copy_from_slice(b"PARQ");

        assert_eq!(unpack(&packed), Err(FormatError::NotAPointSet));
    }

    #[test]
    fn a_later_version_is_rejected_rather_than_misread() {
        let mut packed = pack(&points()).unwrap();
        packed[4..8].copy_from_slice(&(VERSION + 1).to_le_bytes());

        assert_eq!(
            unpack(&packed),
            Err(FormatError::UnsupportedVersion { found: VERSION + 1 })
        );
    }

    #[test]
    fn a_buffer_too_short_for_a_header_is_rejected() {
        assert_eq!(unpack(&[]), Err(FormatError::NoHeader(0)));
        assert_eq!(unpack(&MAGIC), Err(FormatError::NoHeader(4)));
    }

    /// A truncated file would otherwise read as points made of whatever bytes followed, or
    /// panic on the slice that runs off the end.
    #[test]
    fn a_buffer_that_does_not_hold_the_points_it_claims_is_rejected() {
        let packed = pack(&points()).unwrap();

        let cut = &packed[..packed.len() - 4];
        assert_eq!(
            unpack(cut),
            Err(FormatError::Truncated {
                points: 2,
                needed: packed.len(),
                got: cut.len(),
            })
        );

        let mut overclaimed = packed.clone();
        overclaimed[8..12].copy_from_slice(&99u32.to_le_bytes());
        assert!(matches!(
            unpack(&overclaimed),
            Err(FormatError::Truncated { points: 99, .. })
        ));
    }

    #[test]
    fn a_point_carries_its_crossings_position() {
        let crossing = Crossing {
            crossing_id: "water:rail@0.5".parse().expect("id"),
            rail_id: "rail".to_string(),
            water_id: "water".to_string(),
            frac: 0.5,
            position: coord! { x: RUHLAND.0, y: RUHLAND.1 },
            extract_id: "20260727T193628Z".to_string(),
        };
        let id = CrossingId::of(&Key::from(&crossing));

        let point = Point::of(&crossing, id);

        assert_eq!(point.id, id);
        assert_eq!(point.latitude, RUHLAND.1 as f32);
        assert_eq!(point.longitude, RUHLAND.0 as f32);
    }
}
