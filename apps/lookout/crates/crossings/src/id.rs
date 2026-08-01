//! What names a crossing in the packed buffer.
//!
//! The id is derived from what the crossing *is* — the track, the water, and where along the
//! track they meet — rather than from where its row landed. So the same crossing carries the
//! same id after the dataset is rebuilt, after a bbox restricts it to a region, and between a
//! prediction made on the device and a ground truth derived on the laptop, none of which
//! preserve row order.
//!
//! Four bytes is small enough that distinct crossings can collide by chance, so a pack run
//! checks and refuses rather than shipping two crossings the device cannot tell apart.

use std::collections::HashMap;
use std::fmt::{self, Display};

use crate::silver::Crossing;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{first} and {second} are different crossings with the same id {id}")]
pub struct Collision {
    pub id: CrossingId,
    pub first: Key,
    pub second: Key,
}

/// A crossing's id in the packed buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CrossingId(u32);

impl CrossingId {
    /// The id of the crossing this key names.
    pub fn of(key: &Key) -> Self {
        let digest = md5::compute(key.bytes());
        Self(u32::from_le_bytes(
            digest.0[..4].try_into().expect("a digest is 16 bytes"),
        ))
    }

    pub fn get(&self) -> u32 {
        self.0
    }

    /// An id read back from a packed buffer, where only the number itself survives.
    pub fn from_bits(id: u32) -> Self {
        Self(id)
    }
}

impl Display for CrossingId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

/// What makes a crossing that crossing, in the source's own terms.
///
/// `frac` is part of it: one body of water meets the same stretch of track many times over
/// where it meanders, and those are different crossings a train passes at different moments.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    rail_id: String,
    water_id: String,
    /// Held as bits rather than as an `f64` so that a key is comparable and hashable, and so
    /// that hashing it never depends on how a float formats.
    frac: u64,
}

impl Key {
    pub fn new(rail_id: impl Into<String>, water_id: impl Into<String>, frac: f64) -> Self {
        Self {
            rail_id: rail_id.into(),
            water_id: water_id.into(),
            frac: frac.to_bits(),
        }
    }

    /// The bytes an id is derived from. A separator keeps the fields from running together,
    /// so ids stay distinct where one field's tail could otherwise read as the next one's
    /// head; the ids are only reproducible as long as this stays put.
    fn bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.rail_id.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(self.water_id.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&self.frac.to_le_bytes());
        bytes
    }
}

impl From<&Crossing> for Key {
    fn from(crossing: &Crossing) -> Self {
        Self::new(&crossing.rail_id, &crossing.water_id, crossing.frac)
    }
}

impl Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}/{}@{}",
            self.rail_id,
            self.water_id,
            f64::from_bits(self.frac)
        )
    }
}

/// One id per crossing, in the order given.
///
/// Two rows naming the same crossing keep the same id — that is what an identity-derived id
/// means. Two *different* crossings landing on one id is the failure this returns, because
/// the buffer would then hold a name that means two things.
pub fn assign(crossings: &[Crossing]) -> Result<Vec<CrossingId>, Collision> {
    let mut claimed: HashMap<CrossingId, Key> = HashMap::new();

    crossings
        .iter()
        .map(|crossing| {
            let key = Key::from(crossing);
            let id = CrossingId::of(&key);
            match claimed.get(&id) {
                Some(first) if first != &key => Err(Collision {
                    id,
                    first: first.clone(),
                    second: key,
                }),
                _ => {
                    claimed.insert(id, key);
                    Ok(id)
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use geo_types::coord;

    use super::*;

    const RAIL: &str = "86aaefea-fac9-4b5e-9f60-2c19678f07c6";
    const WATER: &str = "e597395d-c46d-3b24-a45f-e85abefc2fb5";

    fn crossing(rail_id: &str, water_id: &str, frac: f64) -> Crossing {
        Crossing {
            crossing_id: format!("{water_id}:{rail_id}@{frac}").parse().expect("id"),
            rail_id: rail_id.to_string(),
            water_id: water_id.to_string(),
            frac,
            position: coord! { x: 13.548209, y: 51.617567 },
            extract_id: "20260727T193628Z".to_string(),
        }
    }

    #[test]
    fn the_same_crossing_always_gets_the_same_id() {
        let key = Key::new(RAIL, WATER, 0.128044);

        assert_eq!(CrossingId::of(&key), CrossingId::of(&key.clone()));
    }

    /// The id has to survive a rebuild, so it cannot depend on anything about the row: not
    /// its index, not what came before it, not how many crossings there are.
    #[test]
    fn an_id_does_not_depend_on_where_the_crossing_sits_in_the_set() {
        let one = crossing(RAIL, WATER, 0.1);
        let other = crossing(RAIL, WATER, 0.9);

        let forwards = assign(&[one.clone(), other.clone()]).unwrap();
        let backwards = assign(&[other, one]).unwrap();
        let alone = assign(&[crossing(RAIL, WATER, 0.9)]).unwrap();

        assert_eq!(forwards, vec![backwards[1], backwards[0]]);
        assert_eq!(alone, vec![forwards[1]]);
    }

    /// The dataset's own shape: a meandering river meets one stretch of track many times, and
    /// each of those is a crossing a train passes at a different moment.
    #[test]
    fn the_same_water_and_track_at_different_points_are_different_crossings() {
        let ids = assign(&[
            crossing(RAIL, WATER, 0.301087),
            crossing(RAIL, WATER, 0.341096),
            crossing(RAIL, WATER, 0.994696),
        ])
        .unwrap();

        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            3
        );
    }

    #[test]
    fn a_different_track_or_a_different_water_is_a_different_crossing() {
        let ids = assign(&[
            crossing(RAIL, WATER, 0.5),
            crossing("other-rail", WATER, 0.5),
            crossing(RAIL, "other-water", 0.5),
        ])
        .unwrap();

        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            3
        );
    }

    /// Nothing stops one field's tail from reading as the next one's head unless the fields
    /// are kept apart, and two crossings that differ only in where the split falls are still
    /// two crossings.
    #[test]
    fn the_fields_of_a_key_cannot_run_together() {
        let ids = assign(&[crossing("ab", "c", 0.5), crossing("a", "bc", 0.5)]).unwrap();

        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn a_repeated_row_keeps_its_id_rather_than_colliding_with_itself() {
        let ids = assign(&[crossing(RAIL, WATER, 0.5), crossing(RAIL, WATER, 0.5)]).unwrap();

        assert_eq!(ids[0], ids[1]);
    }

    /// A real pair of colliding keys, found by search over this exact derivation. They pin
    /// the derivation as much as the refusal: change how an id is derived and these stop
    /// colliding, which is the signal that every id in the field has moved.
    const COLLIDING: [&str; 2] = ["r29320", "r143792"];

    /// Four bytes is few enough that distinct crossings collide by chance, and the device
    /// cannot tell two crossings of one name apart — so a run that would ship such a pair
    /// says so instead of packing it.
    #[test]
    fn two_different_crossings_sharing_an_id_are_refused() {
        let [first, second] = COLLIDING.map(|rail| crossing(rail, "w", 0.5));

        let collision = assign(&[first.clone(), second.clone()]).unwrap_err();

        assert_eq!(collision.first, Key::from(&first));
        assert_eq!(collision.second, Key::from(&second));
        assert_eq!(collision.id, CrossingId::of(&Key::from(&first)));
    }

    #[test]
    fn a_collision_names_both_crossings_and_the_id_they_share() {
        let [first, second] = COLLIDING.map(|rail| crossing(rail, "w", 0.5));

        let reported = assign(&[first, second]).unwrap_err().to_string();

        for named in [COLLIDING[0], COLLIDING[1], "292e417a"] {
            assert!(reported.contains(named), "{reported} does not name {named}");
        }
    }

    #[test]
    fn an_id_reads_as_eight_hex_digits() {
        assert_eq!(CrossingId(0x0000_00ff).to_string(), "000000ff");
    }
}
