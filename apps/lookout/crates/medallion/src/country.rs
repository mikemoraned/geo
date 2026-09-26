use std::fmt::{self, Display};
use std::str::FromStr;

use geo_types::Point;

pub const COUNTRY: &str = "country";

pub trait Countries {
    fn containing(&self, point: Point<f64>) -> Option<Country>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Country {
    Germany,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown country `{code}`; known: {known}", known = Country::codes())]
pub struct UnknownCountry {
    code: String,
}

impl FromStr for Country {
    type Err = UnknownCountry;

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
    pub const ALL: [Country; 1] = [Country::Germany];

    pub fn codes() -> String {
        Country::ALL
            .iter()
            .map(|country| country.code())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn code(self) -> &'static str {
        match self {
            Country::Germany => "DE",
        }
    }

    pub fn projected_epsg(self) -> u16 {
        match self {
            Country::Germany => 25832,
        }
    }

    pub fn projected_projjson(self) -> &'static str {
        match self {
            Country::Germany => include_str!("etrs89_utm32n.projjson.json"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
