use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use object_store::path::Path as ObjectPath;

pub const OVERTURE: &str = "s3://overturemaps-us-west-2/release";

const S3_SCHEME: &str = "s3://";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Location {
    Bucket { bucket: String, prefix: ObjectPath },
    Mirror(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
#[error("{0} names no bucket. A bucket source takes the form s3://<bucket>/<prefix>.")]
pub struct NoBucket(String);

impl FromStr for Location {
    type Err = NoBucket;

    fn from_str(location: &str) -> Result<Self, Self::Err> {
        let Some(bucket_and_prefix) = location.strip_prefix(S3_SCHEME) else {
            return Ok(Self::Mirror(PathBuf::from(location)));
        };
        let (bucket, prefix) = bucket_and_prefix
            .split_once('/')
            .unwrap_or((bucket_and_prefix, ""));
        if bucket.is_empty() {
            return Err(NoBucket(location.to_string()));
        }
        Ok(Self::Bucket {
            bucket: bucket.to_string(),
            prefix: ObjectPath::from(prefix),
        })
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Bucket { bucket, prefix } => write!(f, "{S3_SCHEME}{bucket}/{prefix}"),
            Self::Mirror(path) => write!(f, "{}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(text: &str) -> Location {
        text.parse().expect("a location")
    }

    #[test]
    fn an_s3_url_is_a_bucket_and_the_prefix_its_releases_sit_under() {
        assert_eq!(
            location(OVERTURE),
            Location::Bucket {
                bucket: "overturemaps-us-west-2".to_string(),
                prefix: ObjectPath::from("release"),
            }
        );
    }

    #[test]
    fn an_s3_url_without_a_prefix_has_its_releases_at_the_root() {
        assert_eq!(
            location("s3://somebucket"),
            Location::Bucket {
                bucket: "somebucket".to_string(),
                prefix: ObjectPath::default(),
            }
        );
    }

    #[test]
    fn an_s3_url_without_a_bucket_is_refused() {
        assert_eq!(
            "s3://".parse::<Location>(),
            Err(NoBucket("s3://".to_string()))
        );
    }

    #[test]
    fn anything_else_is_a_mirror() {
        assert_eq!(
            location("/Volumes/portable/release"),
            Location::Mirror(PathBuf::from("/Volumes/portable/release"))
        );
    }

    #[test]
    fn a_bucket_prints_as_its_url() {
        assert_eq!(location(OVERTURE).to_string(), OVERTURE);
    }
}
