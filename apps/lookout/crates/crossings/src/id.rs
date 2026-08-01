//! What names a crossing in the packed buffer.
//!
//! The id is a hash of the crossing's silver id — the name the store gives it — rather than
//! anything derived from where its row landed. So the same crossing carries the same id after
//! the dataset is rebuilt and after a bbox restricts it to a region, neither of which preserves
//! row order, and a prediction made on the device names a crossing a ground truth derived on
//! the laptop can be looked up by.
//!
//! Hashing the silver id, rather than re-deriving one from the columns behind it, is what makes
//! that last part true: the two would otherwise be free to disagree about what a crossing is —
//! and they did, the silver id being keyed on the connected track where a key built from
//! `rail_id` splits one track into a crossing per segment.
//!
//! Four bytes is small enough that distinct crossings can collide by chance, so a pack run
//! checks and refuses rather than shipping two crossings the device cannot tell apart. Four is
//! what the device has room for; widening it is a change to the format it scans.

use std::collections::HashMap;
use std::fmt::{self, Display};

use model::CrossingId;

use crate::silver::Crossing;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{first} and {second} are different crossings with the same id {id}")]
pub struct Collision {
    pub id: PackedId,
    pub first: CrossingId,
    pub second: CrossingId,
}

/// A crossing's id in the packed buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PackedId(u32);

impl PackedId {
    /// The id of the crossing the store calls `crossing_id`.
    pub fn of(crossing_id: &CrossingId) -> Self {
        let digest = md5::compute(crossing_id.to_string().as_bytes());
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

impl Display for PackedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

/// One id per crossing, in the order given.
///
/// Two rows naming the same crossing keep the same id — that is what an identity-derived id
/// means. Two *different* crossings landing on one id is the failure this returns, because
/// the buffer would then hold a name that means two things.
pub fn assign(crossings: &[Crossing]) -> Result<Vec<PackedId>, Collision> {
    let mut claimed: HashMap<PackedId, CrossingId> = HashMap::new();

    crossings
        .iter()
        .map(|crossing| {
            let crossing_id = &crossing.crossing_id;
            let id = PackedId::of(crossing_id);
            match claimed.get(&id) {
                Some(first) if first != crossing_id => Err(Collision {
                    id,
                    first: first.clone(),
                    second: crossing_id.clone(),
                }),
                _ => {
                    claimed.insert(id, crossing_id.clone());
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

    /// A crossing id of the shape the store holds: the track, the water, and where along the
    /// track the two meet.
    const RUHLAND: &str = "3e414321-8593-3d3c-b78c-13e50f60e342:\
        29060dad-d4f4-40e1-a09f-44b779b52eeb:29060dad-d4f4-40e1-a09f-44b779b52eeb@0.890350";

    fn crossing(crossing_id: &str) -> Crossing {
        Crossing {
            crossing_id: crossing_id.parse().expect("id"),
            position: coord! { x: 13.548209, y: 51.617567 },
            extract_id: "20260727T193628Z".to_string(),
        }
    }

    #[test]
    fn the_same_crossing_always_gets_the_same_id() {
        let crossing_id: CrossingId = RUHLAND.parse().unwrap();

        assert_eq!(PackedId::of(&crossing_id), PackedId::of(&crossing_id));
    }

    /// The whole point of hashing the store's own id: a device holding an id can be matched
    /// back to the ground truth, which names crossings the way silver does.
    #[test]
    fn an_id_is_a_function_of_the_crossing_id_and_of_nothing_else() {
        let by_the_row = assign(&[crossing(RUHLAND)]).unwrap()[0];

        assert_eq!(by_the_row, PackedId::of(&RUHLAND.parse().unwrap()));
    }

    /// The id has to survive a rebuild, so it cannot depend on anything about the row: not
    /// its index, not what came before it, not how many crossings there are.
    #[test]
    fn an_id_does_not_depend_on_where_the_crossing_sits_in_the_set() {
        let one = crossing("track:water@0.1");
        let other = crossing("track:water@0.9");

        let forwards = assign(&[one.clone(), other.clone()]).unwrap();
        let backwards = assign(&[other, one]).unwrap();
        let alone = assign(&[crossing("track:water@0.9")]).unwrap();

        assert_eq!(forwards, vec![backwards[1], backwards[0]]);
        assert_eq!(alone, vec![forwards[1]]);
    }

    /// The dataset's own shape: a meandering river meets one stretch of track many times, and
    /// each of those is a crossing a train passes at a different moment. Silver says so by
    /// giving them different ids, and this derivation keeps them apart.
    #[test]
    fn crossings_silver_names_differently_are_different_crossings() {
        let ids = assign(&[
            crossing("track:water@0.301087"),
            crossing("track:water@0.341096"),
            crossing("track:water@0.994696"),
        ])
        .unwrap();

        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            3
        );
    }

    #[test]
    fn a_repeated_row_keeps_its_id_rather_than_colliding_with_itself() {
        let ids = assign(&[crossing("track:water@0.5"), crossing("track:water@0.5")]).unwrap();

        assert_eq!(ids[0], ids[1]);
    }

    /// A real pair of colliding crossing ids, found by search over this exact derivation. They
    /// pin the derivation as much as the refusal: change how an id is derived and these stop
    /// colliding, which is the signal that every id in the field has moved.
    const COLLIDING: [&str; 2] = ["track:water@40737", "track:water@43834"];

    /// Four bytes is few enough that distinct crossings collide by chance, and the device
    /// cannot tell two crossings of one name apart — so a run that would ship such a pair
    /// says so instead of packing it.
    #[test]
    fn two_different_crossings_sharing_an_id_are_refused() {
        let [first, second] = COLLIDING.map(crossing);

        let collision = assign(&[first.clone(), second.clone()]).unwrap_err();

        assert_eq!(collision.first, first.crossing_id);
        assert_eq!(collision.second, second.crossing_id);
        assert_eq!(collision.id, PackedId::of(&first.crossing_id));
    }

    #[test]
    fn a_collision_names_both_crossings_and_the_id_they_share() {
        let [first, second] = COLLIDING.map(crossing);

        let reported = assign(&[first, second]).unwrap_err().to_string();

        for named in [COLLIDING[0], COLLIDING[1], "2490bdfe"] {
            assert!(reported.contains(named), "{reported} does not name {named}");
        }
    }

    #[test]
    fn an_id_reads_as_eight_hex_digits() {
        assert_eq!(PackedId(0x0000_00ff).to_string(), "000000ff");
    }
}
