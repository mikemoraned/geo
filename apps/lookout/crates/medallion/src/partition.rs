//! Hive partition keys and values, and the naming rules they must meet.

use std::fmt::{self, Display};
use std::str::FromStr;

/// Date partition values, per `docs/medallion.md`.
pub(crate) const DATE_FORMAT: &str = "%Y-%m-%d";

/// A key or value that does not meet the store's naming rules.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("partition key `{0}` is not snake_case")]
    Key(String),
    #[error("partition value `{0}` is empty or contains a reserved character")]
    Value(String),
    #[error("dataset `{0}` declares no partition key")]
    Unpartitioned(String),
}

/// The left-hand side of a `key=value` partition directory: snake_case, so it is also a
/// usable column name in every engine that reads the partitioning back.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PartitionKey(String);

impl PartitionKey {
    pub fn new(key: impl Into<String>) -> Result<Self, PathError> {
        let key = key.into();
        let mut chars = key.chars();
        let snake_case = chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if snake_case {
            Ok(Self(key))
        } else {
            Err(PathError::Key(key))
        }
    }
}

impl FromStr for PartitionKey {
    type Err = PathError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl Display for PartitionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The right-hand side of a `key=value` partition directory. Reserved characters are those
/// that would make the directory name ambiguous to a Hive-style path parser.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PartitionValue(String);

impl PartitionValue {
    pub fn new(value: impl Into<String>) -> Result<Self, PathError> {
        let value = value.into();
        if value.is_empty() || value.contains(['/', ' ', '=']) {
            Err(PathError::Value(value))
        } else {
            Ok(Self(value))
        }
    }
}

impl FromStr for PartitionValue {
    type Err = PathError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl Display for PartitionValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// One `key=value` directory.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Partition {
    pub key: PartitionKey,
    pub value: PartitionValue,
}

impl Partition {
    pub fn new(key: &str, value: impl Display) -> Result<Self, PathError> {
        Ok(Self {
            key: PartitionKey::new(key)?,
            value: PartitionValue::new(value.to_string())?,
        })
    }
}

impl Display for Partition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}={}", self.key, self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partition_renders_as_a_hive_directory_name() {
        assert_eq!(
            Partition::new("sensor", "gps").unwrap().to_string(),
            "sensor=gps"
        );
    }

    #[test]
    fn values_are_stringified_from_any_displayable() {
        assert_eq!(
            Partition::new("run_id", 42).unwrap().to_string(),
            "run_id=42"
        );
    }

    #[test]
    fn a_non_snake_case_key_is_rejected() {
        for key in ["ingestedDate", "Ingested_date", "ingested-date", "", "1st"] {
            assert_eq!(
                PartitionKey::new(key).unwrap_err(),
                PathError::Key(key.to_string()),
                "key `{key}` should be rejected"
            );
        }
    }

    #[test]
    fn a_value_containing_a_reserved_character_is_rejected() {
        for value in ["a/b", "a b", "a=b", ""] {
            assert_eq!(
                PartitionValue::new(value).unwrap_err(),
                PathError::Value(value.to_string()),
                "value `{value}` should be rejected"
            );
        }
    }

    #[test]
    fn keys_and_values_parse_from_str() {
        assert_eq!(
            "ingested_date".parse::<PartitionKey>().unwrap().to_string(),
            "ingested_date"
        );
        assert!("Ingested".parse::<PartitionKey>().is_err());
        assert_eq!(
            "2026-07-26".parse::<PartitionValue>().unwrap().to_string(),
            "2026-07-26"
        );
        assert!("a b".parse::<PartitionValue>().is_err());
    }
}
