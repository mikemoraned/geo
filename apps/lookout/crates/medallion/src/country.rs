//! The countries the store holds geometry for, and the projected CRS each one uses.
//!
//! Silver carries a lat/lon geometry in a global CRS and may carry a second one in metres.
//! The rule for that second column is **one projected zone per country**: several UTM zones
//! may cover a country, but a single zone keeps every geometry within it directly
//! comparable. This is where that choice is made, so a dataset states which country's
//! geometry it holds and never picks a zone of its own.

use std::fmt::{self, Display};
use std::str::FromStr;

use geo_types::Point;

/// Somewhere that can say which country a place is in.
///
/// A projected zone is chosen per country, so anything writing projected geometry has to
/// know the country of each thing it writes. That is a property of where the thing is, not
/// of the run that wrote it, so it is looked up rather than passed down.
pub trait Countries {
    /// The country containing `point`, or `None` where that is no country the store knows.
    fn containing(&self, point: Point<f64>) -> Option<Country>;
}

/// A country whose geometry the store can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Country {
    Germany,
}

/// A code naming no country the store knows.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown country `{code}`; known: {known}", known = Country::codes())]
pub struct UnknownCountry {
    code: String,
}

impl FromStr for Country {
    type Err = UnknownCountry;

    /// Parses an ISO 3166-1 alpha-2 code, in either case.
    fn from_str(code: &str) -> Result<Self, Self::Err> {
        Country::ALL
            .into_iter()
            .find(|country| country.code().eq_ignore_ascii_case(code))
            .ok_or_else(|| UnknownCountry {
                code: code.to_string(),
            })
    }
}

impl Display for Country {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.code().fmt(f)
    }
}

impl Country {
    /// Every country the store knows, for checks that must cover all of them.
    pub const ALL: [Country; 1] = [Country::Germany];

    /// The codes of every known country, for an error that lists the choices.
    pub fn codes() -> String {
        Country::ALL
            .iter()
            .map(|country| country.code())
            .collect::<Vec<_>>()
            .join(", ")
    }

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

    /// Round-trips through the code, so a country named on a command line is the one its
    /// partition is named for.
    #[test]
    fn a_country_parses_from_its_own_code_in_either_case() {
        for country in Country::ALL {
            assert_eq!(country.code().parse(), Ok(country));
            assert_eq!(country.code().to_lowercase().parse(), Ok(country));
            assert_eq!(country.to_string(), country.code());
        }
    }

    #[test]
    fn an_unknown_code_is_rejected_with_the_known_ones() {
        let err = "ZZ".parse::<Country>().unwrap_err();

        assert!(err.to_string().contains("ZZ"), "{err}");
        assert!(err.to_string().contains("DE"), "{err}");
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
