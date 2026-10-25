use chrono::{DateTime, TimeZone, Utc};
use medallion_model::{Feed, MotisSource};
use url::Url;

use crate::client::DEFAULT_BASE_URL;

pub fn legacy_sources() -> [MotisSource; 2] {
    [local("gtfs.de"), local("delfi")]
}

pub fn legacy_source_id_sql(captured_at: &str) -> String {
    let [gtfs_de, delfi] = legacy_sources().map(|source| source.id());
    format!(
        "CASE WHEN {captured_at} < TIMESTAMP '{}' THEN '{gtfs_de}' ELSE '{delfi}' END",
        delfi_from().format("%Y-%m-%dT%H:%M:%SZ")
    )
}

fn delfi_from() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 23, 0, 0, 0)
        .single()
        .expect("a fixed instant in UTC is unambiguous")
}

fn local(label: &str) -> MotisSource {
    MotisSource {
        base_url: Url::parse(DEFAULT_BASE_URL).expect("the default base URL is a URL"),
        motis_version: "v2.10.2".parse().expect("a fixed version parses"),
        feed: Feed::Local,
        area: None,
        label: Some(label.parse().expect("a fixed label parses")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_legacy_sources_are_the_local_server_at_v2_10_2_with_no_area() {
        for source in legacy_sources() {
            assert_eq!(
                (source.feed, source.motis_version.to_string(), source.area),
                (Feed::Local, "v2.10.2".to_string(), None)
            );
        }
    }
}
