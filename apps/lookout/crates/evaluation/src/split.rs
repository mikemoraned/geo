use geo_types::Point;
use hashfn::hashfn;
use nutype::nutype;
use sha2::{Digest, Sha256};

use crate::a5::{A5CellId, A5Resolution, NoA5Cell};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Splitter {
    pub resolution: A5Resolution,
    pub salt: Salt,
    pub tune_buckets: TuneBuckets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Split {
    Tune,
    Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SplitterBucket(pub u8);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScoredSplit {
    pub splitter: Splitter,
    pub split: Split,
}

#[nutype(
    validate(with = salt_present, error = EmptySalt),
    derive(Debug, Clone, PartialEq, Eq, Hash, Display, FromStr, TryFrom, Into, AsRef)
)]
pub struct Salt(String);

#[nutype(
    validate(with = tune_buckets_in_range, error = NotTuneBuckets),
    derive(
        Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Display, FromStr, TryFrom, Into
    )
)]
pub struct TuneBuckets(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a salt cannot be empty")]
pub struct EmptySalt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{0} is not a count of buckets, from 0 to 100")]
pub struct NotTuneBuckets(pub u8);

#[hashfn]
pub fn salt_present(salt: &str) -> Result<(), EmptySalt> {
    if salt.is_empty() {
        Err(EmptySalt)
    } else {
        Ok(())
    }
}

#[hashfn]
pub fn tune_buckets_in_range(tune_buckets: &u8) -> Result<(), NotTuneBuckets> {
    if *tune_buckets <= 100 {
        Ok(())
    } else {
        Err(NotTuneBuckets(*tune_buckets))
    }
}

impl Salt {
    pub fn new(salt: impl Into<String>) -> Result<Self, EmptySalt> {
        Self::try_new(salt.into())
    }
}

impl TuneBuckets {
    pub fn new(tune_buckets: u8) -> Result<Self, NotTuneBuckets> {
        Self::try_new(tune_buckets)
    }
}

impl Splitter {
    pub fn cell(&self, point: Point<f64>) -> Result<A5CellId, NoA5Cell> {
        A5CellId::containing(point, self.resolution)
    }

    pub fn bucket(&self, cell: A5CellId) -> SplitterBucket {
        let digest = Sha256::digest(format!("{}:{cell}", self.salt));
        let leading: [u8; 8] = digest[..8]
            .try_into()
            .expect("a SHA-256 digest holds 32 bytes");
        SplitterBucket((u64::from_be_bytes(leading) % 100) as u8)
    }

    pub fn split(&self, cell: A5CellId) -> Split {
        if self.bucket(cell).0 < u8::from(self.tune_buckets) {
            Split::Tune
        } else {
            Split::Test
        }
    }
}

impl ScoredSplit {
    pub fn contains(&self, point: Point<f64>) -> Result<bool, NoA5Cell> {
        self.splitter
            .cell(point)
            .map(|cell| self.splitter.split(cell) == self.split)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proptest::prelude::*;

    use super::*;

    fn splitter(tune_buckets: u8) -> Splitter {
        Splitter {
            resolution: "8".parse().unwrap(),
            salt: "tune-test-1".parse().unwrap(),
            tune_buckets: TuneBuckets::new(tune_buckets).unwrap(),
        }
    }

    fn germany() -> impl Strategy<Value = Point<f64>> {
        (5.9..15.1, 47.3..55.1).prop_map(|(lon, lat)| Point::new(lon, lat))
    }

    #[test]
    fn dresden_hbf_lies_in_a_test_cell_under_tune_test_1() {
        let splitter = splitter(70);
        let cell = splitter.cell(Point::new(13.7322, 51.0403)).unwrap();

        assert_eq!(cell.to_string(), "5156880000000000");
        assert_eq!(splitter.bucket(cell), SplitterBucket(72));
        assert_eq!(splitter.split(cell), Split::Test);
    }

    #[test]
    fn a_splitter_refuses_settings_outside_their_range() {
        assert_eq!(Salt::new(""), Err(EmptySalt));
        assert_eq!(TuneBuckets::new(100).map(u8::from), Ok(100));
        assert_eq!(TuneBuckets::new(101), Err(NotTuneBuckets(101)));
    }

    proptest! {
        #[test]
        fn no_cell_lies_in_both_splits(points in prop::collection::vec(germany(), 1..200)) {
            let splitter = splitter(70);
            let mut splits = BTreeMap::new();
            for point in points {
                let cell = splitter.cell(point).unwrap();
                let split = splitter.split(cell);
                prop_assert_eq!(*splits.entry(cell).or_insert(split), split);
            }
        }

        #[test]
        fn a_cell_takes_one_split_on_every_run(point in germany()) {
            let cell = splitter(70).cell(point).unwrap();

            prop_assert_eq!(splitter(70).split(cell), splitter(70).split(cell));
        }

        #[test]
        fn a_cell_is_tune_when_its_bucket_lies_below_the_tune_buckets(
            point in germany(),
            tune_buckets in 0..=100u8,
        ) {
            let splitter = splitter(tune_buckets);
            let cell = splitter.cell(point).unwrap();
            let SplitterBucket(bucket) = splitter.bucket(cell);
            let expected = if bucket < tune_buckets { Split::Tune } else { Split::Test };

            prop_assert!(bucket < 100);
            prop_assert_eq!(splitter.split(cell), expected);
        }
    }
}
