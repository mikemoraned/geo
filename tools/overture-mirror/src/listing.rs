use crate::release::{NotARelease, Release};

pub const BUCKET: &str = "overturemaps-us-west-2";

pub const PREFIX: &str = "release/";

#[derive(Debug, thiserror::Error)]
pub enum ListingError {
    #[error("reading the bucket listing: {0}")]
    Json(#[from] serde_json::Error),
    #[error("the bucket listing names {0}")]
    Name(#[from] NotARelease),
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Listing {
    common_prefixes: Vec<CommonPrefix>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CommonPrefix {
    prefix: String,
}

pub fn served_in(listing: &str) -> Result<Vec<Release>, ListingError> {
    let listing: Listing = serde_json::from_str(listing)?;
    let mut served = listing
        .common_prefixes
        .iter()
        .map(|entry| {
            Release::new(
                entry
                    .prefix
                    .trim_start_matches(PREFIX)
                    .trim_end_matches('/'),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    served.sort();
    Ok(served)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_served_releases_come_back_in_order() {
        let listing = r#"{"CommonPrefixes": [
            {"Prefix": "release/2026-09-23.1/"},
            {"Prefix": "release/2026-08-19.0/"}
        ]}"#;

        assert_eq!(
            served_in(listing).expect("a listing"),
            vec![
                Release::new("2026-08-19.0").expect("a release"),
                Release::new("2026-09-23.1").expect("a release"),
            ]
        );
    }

    #[test]
    fn a_prefix_that_is_not_a_release_fails_the_listing() {
        let listing = r#"{"CommonPrefixes": [{"Prefix": "release/latest/"}]}"#;

        assert!(matches!(served_in(listing), Err(ListingError::Name(_))));
    }
}
