use std::fmt::Display;

use chrono::{DateTime, Utc};
use geo_types::{Coord, Rect};
use medallion::{Country, Dataset, DatasetSpec, PartitionValue, PathError, Root, Row, layers};
use nutype::nutype;
use serde::{Deserialize, Serialize};

use crate::gers::GersId;
use crate::overture_release::OvertureRelease;

pub const DIVISION_ID: &str = "division_id";

pub const OVERTURE_EXTRACT: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("overture_extract", "extract_id");

pub const EXTRACT_MANIFEST_V3: DatasetSpec<layers::Bronze> =
    DatasetSpec::unpartitioned("extract_manifest_v3");

const COMPACT_UTC: &str = "%Y%m%dT%H%M%SZ";

pub const RECORDED: &str = "SELECT * FROM extract_manifest_v3 ORDER BY extracted_at DESC";

#[nutype(
    validate(with = partition_value, error = medallion::PathError),
    derive(Debug, Clone, PartialEq, Eq, Hash, Display, FromStr)
)]
pub struct ExtractId(String);

fn partition_value(id: &str) -> Result<(), medallion::PathError> {
    PartitionValue::new(id).map(|_| ())
}

impl ExtractId {
    pub fn at(at: DateTime<Utc>) -> Self {
        #[allow(
            clippy::expect_used,
            reason = "a compact UTC instant holds only digits and letters"
        )]
        Self::new(at.format(COMPACT_UTC).to_string())
            .expect("a compact UTC instant holds only digits and letters")
    }

    pub fn new(id: impl Into<String>) -> Result<Self, medallion::PathError> {
        Self::try_new(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OvertureType {
    pub theme: &'static str,
    pub name: &'static str,
}

impl OvertureType {
    const fn new(theme: &'static str, name: &'static str) -> Self {
        Self { theme, name }
    }

    pub const SEGMENT: Self = Self::new("transportation", "segment");
    pub const CONNECTOR: Self = Self::new("transportation", "connector");
    pub const WATER: Self = Self::new("base", "water");
    pub const DIVISION_AREA: Self = Self::new("divisions", "division_area");
    pub const DIVISION: Self = Self::new("divisions", "division");

    pub fn in_extract(
        self,
        root: &Root,
        extract_id: impl Display,
    ) -> Result<Dataset<layers::Bronze>, PathError> {
        root.dataset(OVERTURE_EXTRACT)
            .for_id(extract_id)?
            .partition("theme", self.theme)?
            .partition("type", self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OvertureRegion {
    pub id: GersId,
    pub name: String,
}

pub fn division_id(country: Country) -> GersId {
    let id = match country {
        Country::Germany => "567d1698-7209-4b94-b7b8-0bc71bde0104",
        Country::UnitedKingdom => "ce3429b1-d5c6-4763-91e7-0107e26d613e",
    };
    #[allow(
        clippy::expect_used,
        reason = "each id above is a constant, checked by the tests below"
    )]
    GersId::new(id).expect("a division id written here is a constant, checked by the tests below")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractManifestRowV3 {
    pub extract_id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub extracted_at: DateTime<Utc>,
    pub overture_release: OvertureRelease,
    /// The code of the country the extract was taken for, as text rather than a [`Country`].
    ///
    /// Bronze records what was observed, so a row naming a country the store has no zone for
    /// still fills in. See
    /// [docs/overture.md](../../../docs/overture.md#taking-an-extract-reads-the-country-division-filling-one-in-reads-the-manifest).
    pub country: String,
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
}

impl ExtractManifestRowV3 {
    pub fn bbox(&self) -> Rect<f64> {
        Rect::new(
            Coord {
                x: self.min_lon,
                y: self.min_lat,
            },
            Coord {
                x: self.max_lon,
                y: self.max_lat,
            },
        )
    }
}

impl Row for ExtractManifestRowV3 {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = EXTRACT_MANIFEST_V3;
    const INSTANTS: &'static [&'static str] = &["extracted_at"];

    #[allow(
        clippy::expect_used,
        reason = "every value here is a constant its type accepts"
    )]
    fn example() -> Self {
        Self {
            extract_id: "example".to_string(),
            extracted_at: DateTime::UNIX_EPOCH,
            overture_release: OvertureRelease::new("2026-09-23.1")
                .expect("a constant release follows Overture's naming"),
            country: "DE".to_string(),
            min_lon: 0.0,
            min_lat: 0.0,
            max_lon: 1.0,
            max_lat: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_country_names_a_division_that_parses_as_a_gers_id() {
        for country in Country::ALL {
            let id = division_id(country);

            assert_eq!(id.to_string().parse(), Ok(id), "{country}");
        }
    }

    #[test]
    fn no_two_countries_name_the_same_division() {
        let mut ids: Vec<String> = Country::ALL
            .into_iter()
            .map(|country| division_id(country).to_string())
            .collect();
        ids.sort_unstable();
        let named = ids.len();
        ids.dedup();

        assert_eq!(
            ids.len(),
            named,
            "two countries share a division id: {ids:?}"
        );
    }
}
