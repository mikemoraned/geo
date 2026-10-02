use chrono::{DateTime, Utc};
use medallion::{Country, DatasetSpec, Row, layers};
use serde::{Deserialize, Serialize};

use crate::gers::GersId;

pub const DIVISION_ID: &str = "division_id";

pub const OVERTURE_EXTRACT: DatasetSpec<layers::Bronze> =
    DatasetSpec::partitioned("overture_extract", "extract_id");

pub const EXTRACT_MANIFEST: DatasetSpec<layers::Bronze> =
    DatasetSpec::unpartitioned("extract_manifest");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractManifestRow {
    pub extract_id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub extracted_at: DateTime<Utc>,
    pub release: String,
    pub country: String,
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
}

pub fn division_id(country: Country) -> GersId {
    let id = match country {
        Country::Germany => "567d1698-7209-4b94-b7b8-0bc71bde0104",
        Country::UnitedKingdom => "ce3429b1-d5c6-4763-91e7-0107e26d613e",
    };
    GersId::new(id).expect("a division id written here is a constant, checked by the tests below")
}

impl Row for ExtractManifestRow {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = EXTRACT_MANIFEST;
    const INSTANTS: &'static [&'static str] = &["extracted_at"];
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
