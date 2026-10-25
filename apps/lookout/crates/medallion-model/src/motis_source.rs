use std::fmt::{self, Display};
use std::str::FromStr;

use chrono::{DateTime, Utc};
use hashfn::hashfn;
use medallion::{Country, DatasetSpec, Row, layers};
use semver::Version;
use serde::{Deserialize, Serialize, Serializer};
use url::Url;
use uuid::Uuid;

use crate::gers::GersId;
use crate::overture::OvertureRegion;
use crate::overture_release::OvertureRelease;

const MOTIS_SOURCE_NAMESPACE: Uuid = Uuid::from_u128(0x3c41_9e2d_7a58_4b06_8f13_d2e6_5a90_c71b);

pub const MOTIS_SOURCE_V3: DatasetSpec<layers::Bronze> =
    DatasetSpec::unpartitioned("motis_source_v3");

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(from = "String")]
pub struct MotisSourceId(String);

impl MotisSourceId {
    #[hashfn]
    pub(crate) fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl From<String> for MotisSourceId {
    fn from(id: String) -> Self {
        Self::new(id)
    }
}

impl Display for MotisSourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct MotisVersion(Version);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{text}` is not a Motis version, such as v2.11.3")]
pub struct NotAMotisVersion {
    text: String,
}

impl MotisVersion {
    #[hashfn]
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

impl Serialize for MotisVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct("MotisVersion", &self.to_string())
    }
}

impl Display for MotisVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct MotisBaseUrl(Url);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{text}` is not a URL: {source}")]
pub struct NotAMotisBaseUrl {
    text: String,
    #[source]
    source: url::ParseError,
}

impl MotisBaseUrl {
    #[hashfn]
    pub fn new(text: impl Into<String>) -> Result<Self, NotAMotisBaseUrl> {
        let text = text.into();
        Url::parse(&text)
            .map(Self)
            .map_err(|source| NotAMotisBaseUrl { text, source })
    }
}

impl FromStr for MotisBaseUrl {
    type Err = NotAMotisBaseUrl;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

impl TryFrom<String> for MotisBaseUrl {
    type Error = NotAMotisBaseUrl;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::new(text)
    }
}

impl From<Url> for MotisBaseUrl {
    fn from(url: Url) -> Self {
        Self(url)
    }
}

impl Serialize for MotisBaseUrl {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct("MotisBaseUrl", self.0.as_str())
    }
}

impl Display for MotisBaseUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct Label(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "`{text}` is not a label: use lowercase letters and digits, with a single `-` or `.` between \
    words, such as delfi-2026-10-12 or gtfs.de"
)]
pub struct NotALabel {
    text: String,
}

impl Label {
    #[hashfn]
    pub fn new(text: impl Into<String>) -> Result<Self, NotALabel> {
        let text = text.into();
        let words_ok = text.split(['-', '.']).all(|word| {
            !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        });
        if words_ok {
            Ok(Self(text))
        } else {
            Err(NotALabel { text })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Label {
    type Err = NotALabel;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

impl TryFrom<String> for Label {
    type Error = NotALabel;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::new(text)
    }
}

impl From<Label> for String {
    fn from(label: Label) -> Self {
        label.0
    }
}

impl Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
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
pub struct Area {
    pub country: Country,
    pub overture_region: Option<OvertureRegion>,
    pub overture_release: OvertureRelease,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MotisSource {
    pub base_url: Url,
    pub motis_version: MotisVersion,
    pub feed: Feed,
    pub area: Option<Area>,
    pub label: Option<Label>,
}

impl MotisSource {
    pub fn id(&self) -> MotisSourceId {
        let country = self.area.as_ref().map_or("", |area| area.country.code());
        let region = self
            .area
            .as_ref()
            .and_then(|area| area.overture_region.as_ref())
            .map(|region| region.id.to_string())
            .unwrap_or_default();
        let version = self.motis_version.to_string();
        let release = self
            .area
            .as_ref()
            .map(|area| area.overture_release.to_string());
        let name = [
            self.base_url.as_str(),
            version.as_str(),
            self.feed.as_str(),
            country,
            region.as_str(),
        ]
        .into_iter()
        .chain(release.as_deref())
        .chain(self.label.as_ref().map(Label::as_str))
        .collect::<Vec<_>>()
        .join("\n");
        MotisSourceId::new(Uuid::new_v5(&MOTIS_SOURCE_NAMESPACE, name.as_bytes()).to_string())
    }

    pub fn row(&self, registered_at: DateTime<Utc>) -> MotisSourceRowV3 {
        let region = self
            .area
            .as_ref()
            .and_then(|area| area.overture_region.as_ref());
        MotisSourceRowV3 {
            source_id: self.id(),
            base_url: MotisBaseUrl::from(self.base_url.clone()),
            motis_version: self.motis_version.clone(),
            feed: self.feed,
            country: self
                .area
                .as_ref()
                .map(|area| area.country.code().to_string()),
            overture_region_id: region.map(|region| region.id),
            overture_region_name: region.map(|region| region.name.clone()),
            overture_release: self.area.as_ref().map(|area| area.overture_release),
            label: self.label.clone(),
            registered_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotisSourceRowV3 {
    pub source_id: MotisSourceId,
    pub base_url: MotisBaseUrl,
    pub motis_version: MotisVersion,
    pub feed: Feed,
    /// The ISO 3166-1 alpha-2 code of the area's country, as text rather than a [`Country`].
    ///
    /// A source naming a country the store has no zone for still reads. See
    /// [docs/motis.md](../../../docs/motis.md#a-capture-names-the-server-that-answered-it).
    pub country: Option<String>,
    pub overture_region_id: Option<GersId>,
    pub overture_region_name: Option<String>,
    pub overture_release: Option<OvertureRelease>,
    pub label: Option<Label>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub registered_at: DateTime<Utc>,
}

impl Row for MotisSourceRowV3 {
    type Layer = layers::Bronze;
    const DATASET: DatasetSpec<Self::Layer> = MOTIS_SOURCE_V3;
    const INSTANTS: &'static [&'static str] = &["registered_at"];

    fn example() -> Self {
        Self {
            source_id: MotisSourceId::new(Uuid::nil().to_string()),
            base_url: MotisBaseUrl::new("https://api.transitous.org/")
                .expect("a constant URL parses"),
            motis_version: MotisVersion(Version::new(2, 11, 3)),
            feed: Feed::Transitous,
            country: Some("DE".to_string()),
            overture_region_id: Some(
                GersId::new(Uuid::nil().to_string()).expect("a constant UUID is a GERS id"),
            ),
            overture_region_name: Some("Thüringen".to_string()),
            overture_release: Some(
                OvertureRelease::new("2026-09-23.1").expect("a constant Overture release parses"),
            ),
            label: Some(Label("example".to_string())),
            registered_at: DateTime::UNIX_EPOCH,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::datatypes::{DataType, Field, TimeUnit};

    use super::*;
    use crate::gers::GersId;
    use crate::testing::column;

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
                overture_region: Some(OvertureRegion {
                    id: GersId::new(REGION_ID).expect("a GERS id"),
                    name: "Thüringen".to_string(),
                }),
                overture_release: release("2026-09-23.1"),
            }),
            label: None,
        }
    }

    fn label(text: &str) -> Label {
        text.parse().expect("a label")
    }

    fn release(text: &str) -> OvertureRelease {
        text.parse().expect("an Overture release")
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
    fn a_label_of_lowercase_words_and_digits_joined_by_single_dashes_is_accepted() {
        for text in [
            "delfi",
            "foo-blah-blah",
            "delfi-2026-10-12",
            "gtfs-de",
            "v2",
        ] {
            assert_eq!(Label::new(text).map(String::from), Ok(text.to_string()));
        }
    }

    #[test]
    fn a_label_with_capitals_spaces_or_stray_separators_is_refused() {
        for text in [
            "",
            "Foo-blah-blah",
            "foo-blah--blah",
            "Foo Blah Blah",
            "foo blah",
            "-foo",
            "foo-",
            "-",
            "foo_blah",
            "foo..blah",
            "foo.-blah",
            ".foo",
            "foo.",
            ".",
            "straße",
        ] {
            assert!(Label::new(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_label_is_checked_when_it_is_read_back() {
        assert!(serde_json::from_str::<Label>(r#""delfi-2026""#).is_ok());
        assert!(serde_json::from_str::<Label>(r#""Delfi 2026""#).is_err());
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
                    overture_region: None,
                    overture_release: release("2026-09-23.1"),
                }),
                ..source.clone()
            },
            MotisSource {
                area: Some(Area {
                    overture_release: release("2026-07-22.0"),
                    ..source.area.clone().expect("an area")
                }),
                ..source.clone()
            },
            MotisSource {
                label: Some(label("delfi")),
                ..source.clone()
            },
        ];

        for other in changed {
            assert_ne!(other.id(), source.id(), "{other:?}");
        }
    }

    #[test]
    fn a_source_without_a_label_keeps_the_id_recorded_before_labels() {
        let thuringia = MotisSource {
            area: Some(Area {
                country: Country::Germany,
                overture_region: Some(OvertureRegion {
                    id: GersId::new("e0ff5492-5758-4335-aabd-af1747925cd1").expect("a GERS id"),
                    name: "Thüringen".to_string(),
                }),
                overture_release: release("2026-09-23.1"),
            }),
            ..transitous()
        };

        assert_eq!(
            thuringia.id().to_string(),
            "dbc79993-2a04-51e0-a6f4-245ab1dfe468"
        );
    }

    #[test]
    fn a_label_tells_apart_sources_with_and_without_an_area() {
        let local = MotisSource {
            area: None,
            label: Some(label("de")),
            ..transitous()
        };
        let area_only = MotisSource {
            area: Some(Area {
                country: Country::Germany,
                overture_region: None,
                overture_release: release("2026-09-23.1"),
            }),
            ..transitous()
        };

        assert_ne!(local.id(), area_only.id());
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
        assert_eq!(
            row.overture_region_id,
            Some(GersId::new(REGION_ID).expect("a GERS id"))
        );
        assert_eq!(row.overture_region_name.as_deref(), Some("Thüringen"));
        assert_eq!(row.overture_release, Some(release("2026-09-23.1")));
        assert_eq!(row.label, None);
        assert_eq!(row.registered_at, instant());
    }

    #[test]
    fn a_source_with_no_area_keeps_the_id_it_had_before_areas_carried_a_release() {
        let local = MotisSource {
            base_url: Url::parse("http://127.0.0.1:8080").expect("a URL"),
            motis_version: version("v2.11.3"),
            feed: Feed::Local,
            area: None,
            label: None,
        };

        assert_eq!(
            local.id().to_string(),
            Uuid::new_v5(
                &MOTIS_SOURCE_NAMESPACE,
                "http://127.0.0.1:8080/\nv2.11.3\nlocal\n\n".as_bytes()
            )
            .to_string()
        );
    }

    #[test]
    fn a_source_row_is_checked_when_it_is_read_back() {
        let row = serde_json::to_value(transitous().row(instant())).expect("a row");

        for (column, refused) in [
            ("base_url", "not a url"),
            ("motis_version", "2.11.3"),
            ("overture_region_id", "no-such-region"),
            ("overture_release", "2026-09-23"),
            ("label", "Delfi 2026"),
        ] {
            let mut read = row.clone();
            read[column] = serde_json::json!(refused);
            assert!(
                serde_json::from_value::<MotisSourceRowV3>(read).is_err(),
                "{column}"
            );
        }
    }

    #[test]
    fn a_source_row_keeps_the_columns_it_is_stored_with() {
        let text = |name, nullable| Field::new(name, DataType::LargeUtf8, nullable);

        assert_eq!(
            medallion::fields::<MotisSourceRowV3>().expect("describe the rows"),
            [
                text("source_id", false),
                text("base_url", false),
                text("motis_version", false),
                text("feed", false),
                text("country", true),
                text("overture_region_id", true),
                text("overture_region_name", true),
                text("overture_release", true),
                text("label", true),
                Field::new(
                    "registered_at",
                    DataType::Timestamp(TimeUnit::Millisecond, Some("UTC".into())),
                    true
                ),
            ]
            .map(Arc::new)
        );
    }

    #[test]
    fn the_feed_is_a_string_column() {
        let feed = column::<MotisSourceRowV3>("feed");

        assert!(
            matches!(feed.data_type(), DataType::Utf8 | DataType::LargeUtf8),
            "{feed:?}"
        );
    }
}
