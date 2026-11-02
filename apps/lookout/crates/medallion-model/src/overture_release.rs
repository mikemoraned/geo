use std::fmt::{self, Display};
use std::str::FromStr;

use chrono::NaiveDate;
use hashfn::hashfn;
use serde::{Deserialize, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub struct OvertureRelease {
    date: NaiveDate,
    revision: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{text}` is not an Overture release, such as 2026-09-23.1")]
pub struct NotAnOvertureRelease {
    text: String,
}

impl OvertureRelease {
    #[hashfn]
    pub fn new(text: impl Into<String>) -> Result<Self, NotAnOvertureRelease> {
        let text = text.into();
        text.split_once('.')
            .and_then(|(date, revision)| {
                Some(Self {
                    date: NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?,
                    revision: revision.parse().ok()?,
                })
            })
            .filter(|release| release.to_string() == text)
            .ok_or(NotAnOvertureRelease { text })
    }
}

impl FromStr for OvertureRelease {
    type Err = NotAnOvertureRelease;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

impl TryFrom<String> for OvertureRelease {
    type Error = NotAnOvertureRelease;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::new(text)
    }
}

impl From<OvertureRelease> for String {
    fn from(release: OvertureRelease) -> Self {
        release.to_string()
    }
}

impl Serialize for OvertureRelease {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct("OvertureRelease", &self.to_string())
    }
}

impl Display for OvertureRelease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.date.format("%Y-%m-%d"), self.revision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_is_written_back_as_overture_names_it() {
        for text in ["2026-07-22.0", "2026-09-23.1", "2026-09-23.12"] {
            assert_eq!(
                OvertureRelease::new(text).map(|release| release.to_string()),
                Ok(text.to_string())
            );
        }
    }

    #[test]
    fn a_release_without_its_revision_or_a_real_date_is_refused() {
        for text in [
            "2026-09-23",
            "2026-09-23.",
            "2026-09-23.a",
            "2026-09-23.01",
            "2026-09-23.+1",
            "2026-9-23.0",
            "2026-13-23.0",
            "26-09-23.0",
            "",
            "latest",
        ] {
            assert!(OvertureRelease::new(text).is_err(), "{text}");
        }
    }

    #[test]
    fn releases_order_by_date_then_revision() {
        let release = |text: &str| OvertureRelease::new(text).expect("a release");

        assert!(release("2026-07-22.0") < release("2026-09-23.0"));
        assert!(release("2026-09-23.0") < release("2026-09-23.1"));
        assert!(release("2026-09-23.2") < release("2026-09-23.10"));
    }

    #[test]
    fn a_release_is_checked_when_it_is_read_back() {
        assert!(serde_json::from_str::<OvertureRelease>(r#""2026-09-23.1""#).is_ok());
        assert!(serde_json::from_str::<OvertureRelease>(r#""2026-09-23""#).is_err());
    }
}
