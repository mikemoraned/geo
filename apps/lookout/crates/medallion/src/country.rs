//! The countries the store holds geometry for, and the projected CRS each one uses.
//!
//! Silver carries a lat/lon geometry in a global CRS and may carry a second one in metres.
//! The rule for that second column is **one projected zone per country**: several UTM zones
//! may cover a country, but a single zone keeps every geometry within it directly
//! comparable. This is where that choice is made, so a dataset states which country's
//! geometry it holds and never picks a zone of its own.

/// A country whose geometry the store can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Country {
    Germany,
}

impl Country {
    /// Every country the store knows, for checks that must cover all of them.
    pub const ALL: [Country; 1] = [Country::Germany];

    /// The ISO 3166-1 alpha-2 code, as used in a `country=` partition.
    pub fn code(self) -> &'static str {
        match self {
            Country::Germany => "DE",
        }
    }

    /// EPSG code of the country's projected CRS.
    pub fn projected_epsg(self) -> u16 {
        match self {
            Country::Germany => 25832,
        }
    }

    /// The country's projected CRS as PROJJSON, the encoding GeoParquet requires.
    /// Generated from PROJ by `just crs-definitions`.
    pub fn projected_projjson(self) -> &'static str {
        match self {
            Country::Germany => include_str!("etrs89_utm32n.projjson.json"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bundled definition is the CRS the code claims it is — a mismatch here would
    /// project geometry correctly but label it wrongly in the file metadata, or the
    /// reverse.
    #[test]
    fn each_country_bundles_the_projjson_of_the_epsg_it_names() {
        for country in Country::ALL {
            let projjson: serde_json::Value =
                serde_json::from_str(country.projected_projjson()).unwrap();

            assert_eq!(projjson["id"]["authority"], "EPSG", "{country:?}");
            assert_eq!(
                projjson["id"]["code"],
                country.projected_epsg(),
                "{country:?}"
            );
            assert_eq!(projjson["type"], "ProjectedCRS", "{country:?}");
        }
    }

    #[test]
    fn a_country_code_is_iso_3166_1_alpha_2() {
        for country in Country::ALL {
            let code = country.code();

            assert_eq!(code.len(), 2, "{country:?}");
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase()),
                "{country:?}: {code}"
            );
        }
    }
}
