use chrono::{DateTime, Utc};
use geo_types::{Coord, Rect};
use medallion::{Country, DatasetSpec, Row, layers};
use serde::{Deserialize, Serialize};

use crate::gers::GersId;

pub const DIVISION_ID: &str = "division_id";

pub const OVERTURE_EXTRACT: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("overture_extract", "extract_id");

pub const EXTRACT_MANIFEST_V2: DatasetSpec<layers::Bronze> =
    DatasetSpec::unpartitioned("extract_manifest_v2");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractManifestRowV2 {
    pub extract_id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub extracted_at: DateTime<Utc>,
    pub overture_release: String,
    pub country: String,
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
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
    GersId::new(id).expect("a division id written here is a constant, checked by the tests below")
}

impl ExtractManifestRowV2 {
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

impl Row for ExtractManifestRowV2 {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = EXTRACT_MANIFEST_V2;
    const INSTANTS: &'static [&'static str] = &["extracted_at"];

    fn example() -> Self {
        Self {
            extract_id: "example".to_string(),
            extracted_at: DateTime::UNIX_EPOCH,
            overture_release: "2026-09-23.1".to_string(),
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
