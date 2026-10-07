use std::fmt::{self, Display};
use std::str::FromStr;

use chrono::{DateTime, Utc};
use medallion::{Country, DatasetSpec, Row, UnknownCountry, layers};
use semver::Version;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::gers::{GersId, NotAGersId};

const MOTIS_SOURCE_NAMESPACE: Uuid = Uuid::from_u128(0x3c41_9e2d_7a58_4b06_8f13_d2e6_5a90_c71b);

pub const MOTIS_SOURCE: DatasetSpec<layers::Bronze> = DatasetSpec::unpartitioned("motis_source");

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MotisSourceId(String);

impl Display for MotisSourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MotisVersion(Version);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{text}` is not a Motis version, such as v2.11.3")]
pub struct NotAMotisVersion {
    text: String,
}

impl MotisVersion {
    pub fn new(text: impl Into<String>) -> Result<Self, NotAMotisVersion> {
        let text = text.into();
        text.strip_prefix('v')
            .and_then(|number| Version::parse(number).ok())
            .map(Self)
            .ok_or(NotAMotisVersion { text })
    }
}

impl FromStr for MotisVersion {
    type Err = NotAMotisVersion;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

impl TryFrom<String> for MotisVersion {
    type Error = NotAMotisVersion;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::new(text)
    }
}

impl From<MotisVersion> for String {
    fn from(version: MotisVersion) -> Self {
        version.to_string()
    }
}

impl Display for MotisVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feed {
    Transitous,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown feed `{text}`; known: {known}", known = Feed::names())]
pub struct UnknownFeed {
    text: String,
}

impl Feed {
    pub const ALL: [Feed; 2] = [Feed::Transitous, Feed::Local];

    pub fn names() -> String {
        Feed::ALL
            .iter()
            .map(|feed| feed.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Feed::Transitous => "transitous",
            Feed::Local => "local",
        }
    }
}

impl FromStr for Feed {
    type Err = UnknownFeed;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Feed::ALL
            .into_iter()
            .find(|feed| feed.as_str() == text)
            .ok_or_else(|| UnknownFeed {
                text: text.to_string(),
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub id: GersId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Area {
    pub country: Country,
    pub region: Option<Region>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MotisSource {
    pub base_url: Url,
    pub motis_version: MotisVersion,
    pub feed: Feed,
    pub area: Option<Area>,
}

#[derive(Debug, thiserror::Error)]
pub enum NotAMotisSource {
    #[error("the base URL `{text}` does not parse: {source}")]
    BaseUrl {
        text: String,
        #[source]
        source: url::ParseError,
    },
    #[error(transparent)]
    Version(#[from] NotAMotisVersion),
    #[error(transparent)]
    Country(#[from] UnknownCountry),
    #[error(transparent)]
    Region(#[from] NotAGersId),
    #[error("a region needs both an id and a name, and the row holds only one")]
    HalfARegion,
    #[error("a region needs a country, and the row holds none")]
    RegionWithoutCountry,
}

impl MotisSource {
    pub fn id(&self) -> MotisSourceId {
        let country = self.area.as_ref().map_or("", |area| area.country.code());
        let region = self
            .area
            .as_ref()
            .and_then(|area| area.region.as_ref())
            .map(|region| region.id.to_string())
            .unwrap_or_default();
        let version = self.motis_version.to_string();
        let name = [
            self.base_url.as_str(),
            version.as_str(),
            self.feed.as_str(),
            country,
            region.as_str(),
        ]
        .join("\n");
        MotisSourceId(Uuid::new_v5(&MOTIS_SOURCE_NAMESPACE, name.as_bytes()).to_string())
    }

    pub fn row(&self, registered_at: DateTime<Utc>) -> MotisSourceRow {
        let region = self.area.as_ref().and_then(|area| area.region.as_ref());
        MotisSourceRow {
            source_id: self.id(),
            base_url: self.base_url.to_string(),
            motis_version: self.motis_version.to_string(),
            feed: self.feed,
            country: self
                .area
                .as_ref()
                .map(|area| area.country.code().to_string()),
            region_id: region.map(|region| region.id.to_string()),
            region_name: region.map(|region| region.name.clone()),
            registered_at,
        }
    }
}

impl TryFrom<MotisSourceRow> for MotisSource {
    type Error = NotAMotisSource;

    fn try_from(row: MotisSourceRow) -> Result<Self, Self::Error> {
        let base_url = Url::parse(&row.base_url).map_err(|source| NotAMotisSource::BaseUrl {
            text: row.base_url.clone(),
            source,
        })?;
        let region = match (row.region_id, row.region_name) {
            (Some(id), Some(name)) => Some(Region {
                id: id.parse()?,
                name,
            }),
            (None, None) => None,
            _ => return Err(NotAMotisSource::HalfARegion),
        };
        let area = match (row.country, region) {
            (Some(country), region) => Some(Area {
                country: country.parse()?,
                region,
            }),
            (None, None) => None,
            (None, Some(_)) => return Err(NotAMotisSource::RegionWithoutCountry),
        };
        Ok(Self {
            base_url,
            motis_version: row.motis_version.parse()?,
            feed: row.feed,
            area,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotisSourceRow {
    pub source_id: MotisSourceId,
    pub base_url: String,
    pub motis_version: String,
    pub feed: Feed,
    pub country: Option<String>,
    pub region_id: Option<String>,
    pub region_name: Option<String>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub registered_at: DateTime<Utc>,
}

impl Row for MotisSourceRow {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = MOTIS_SOURCE;
    const INSTANTS: &'static [&'static str] = &["registered_at"];
}

#[cfg(test)]
mod tests {
    use arrow::datatypes::DataType;

    use super::*;

    const REGION_ID: &str = "0fbd2c80-46c8-4e2b-9ab6-36d2a5c4b0a1";

    fn version(text: &str) -> MotisVersion {
        text.parse().expect("a Motis version")
    }

    fn transitous() -> MotisSource {
        MotisSource {
            base_url: Url::parse("https://api.transitous.org").expect("a URL"),
            motis_version: version("v2.11.3"),
            feed: Feed::Transitous,
            area: Some(Area {
                country: Country::Germany,
                region: Some(Region {
                    id: GersId::new(REGION_ID).expect("a GERS id"),
                    name: "Thüringen".to_string(),
                }),
            }),
        }
    }

    fn instant() -> DateTime<Utc> {
        DateTime::from_timestamp_millis(1_791_270_000_000).expect("an instant")
    }

    #[test]
    fn a_version_is_written_back_as_motis_reports_it() {
        assert_eq!(version("v2.11.3").to_string(), "v2.11.3");
    }

    #[test]
    fn a_version_without_its_v_or_a_part_is_refused() {
        for text in ["2.11.3", "v2.11", "v", "", "vx.y.z", "release-2.11.3"] {
            assert!(MotisVersion::new(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_version_is_checked_when_it_is_read_back() {
        assert!(serde_json::from_str::<MotisVersion>(r#""v2.11.3""#).is_ok());
        assert!(serde_json::from_str::<MotisVersion>(r#""2.11.3""#).is_err());
    }

    #[test]
    fn what_is_refused_is_reported_with_the_text_offered() {
        let err = MotisVersion::new("2.11.3").expect_err("no Motis version");

        assert!(err.to_string().contains("2.11.3"), "{err}");
    }

    #[test]
    fn a_feed_parses_from_the_name_it_is_written_as() {
        for feed in Feed::ALL {
            assert_eq!(feed.as_str().parse(), Ok(feed));
            assert_eq!(
                serde_json::to_string(&feed).expect("a feed serialises"),
                format!("\"{}\"", feed.as_str())
            );
        }
    }

    #[test]
    fn an_unknown_feed_is_refused_with_the_known_ones() {
        let err = "delfi".parse::<Feed>().expect_err("no such feed");

        assert!(err.to_string().contains("delfi"), "{err}");
        for feed in Feed::ALL {
            assert!(err.to_string().contains(feed.as_str()), "{err}");
        }
    }

    #[test]
    fn a_source_has_the_same_id_however_often_it_is_named() {
        assert_eq!(transitous().id(), transitous().id());
    }

    #[test]
    fn each_field_of_a_source_changes_its_id() {
        let source = transitous();
        let changed = [
            MotisSource {
                base_url: Url::parse("http://127.0.0.1:8080").expect("a URL"),
                ..source.clone()
            },
            MotisSource {
                motis_version: version("v2.12.0"),
                ..source.clone()
            },
            MotisSource {
                feed: Feed::Local,
                ..source.clone()
            },
            MotisSource {
                area: None,
                ..source.clone()
            },
            MotisSource {
                area: Some(Area {
                    country: Country::Germany,
                    region: None,
                }),
                ..source.clone()
            },
        ];

        for other in changed {
            assert_ne!(other.id(), source.id(), "{other:?}");
        }
    }

    #[test]
    fn a_source_id_is_a_uuid() {
        let id = transitous().id().to_string();

        assert!(Uuid::parse_str(&id).is_ok(), "{id}");
    }

    #[test]
    fn a_source_row_records_its_area_as_columns() {
        let row = transitous().row(instant());

        assert_eq!(row.source_id, transitous().id());
        assert_eq!(row.country.as_deref(), Some("DE"));
        assert_eq!(row.region_id.as_deref(), Some(REGION_ID));
        assert_eq!(row.region_name.as_deref(), Some("Thüringen"));
        assert_eq!(row.registered_at, instant());
    }

    #[test]
    fn a_source_read_back_from_its_row_is_the_source_written() {
        let source = transitous();

        assert_eq!(
            MotisSource::try_from(source.row(instant())).ok(),
            Some(source)
        );
    }

    #[test]
    fn a_row_holding_a_bad_url_or_version_is_refused() {
        let row = transitous().row(instant());

        let bad_url = MotisSourceRow {
            base_url: "not a url".to_string(),
            ..row.clone()
        };
        let bad_version = MotisSourceRow {
            motis_version: "2.11.3".to_string(),
            ..row
        };

        assert!(MotisSource::try_from(bad_url).is_err());
        assert!(MotisSource::try_from(bad_version).is_err());
    }

    #[test]
    fn the_feed_is_a_string_column() {
        let feed = medallion::fields::<MotisSourceRow>()
            .expect("describe the rows")
            .iter()
            .find(|field| field.name() == "feed")
            .expect("a feed column")
            .data_type()
            .clone();

        assert!(
            matches!(feed, DataType::Utf8 | DataType::LargeUtf8),
            "{feed:?}"
        );
    }
}
