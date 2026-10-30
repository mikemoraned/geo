use chrono::Utc;
use clap::Parser;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

use medallion::{MedallionArgs, Query};
use medallion_model::{
    Area, MOTIS_SOURCE_V3, MotisSource, MotisSourceRowV3, MotisSourceRowV4, OvertureRegion,
};
use motis::source::recorded;

const OLD_NAMESPACE: Uuid = Uuid::from_u128(0x3c41_9e2d_7a58_4b06_8f13_d2e6_5a90_c71b);

#[derive(Parser)]
#[command(about = "Copy bronze motis_source_v3 into motis_source_v4, with each id recomputed")]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[derive(Debug, thiserror::Error)]
enum Refused {
    #[error("{held} differs from its fields' id under the old namespace, {computed}")]
    OldId { held: String, computed: String },
    #[error("source {id} has a country without a release, or a release without a country")]
    Area { id: String },
    #[error("source {id} has a region id without a name, or a name without an id")]
    Region { id: String },
    #[error("source {id}: {source}")]
    Country {
        id: String,
        source: medallion::UnknownCountry,
    },
    #[error("source {id}: {source}")]
    BaseUrl { id: String, source: url::ParseError },
    #[error("motis_source_v4 already holds {rows} rows")]
    AlreadyMigrated { rows: usize },
}

fn old_id(row: &MotisSourceRowV3) -> String {
    let region = row
        .overture_region_id
        .map(|id| id.to_string())
        .unwrap_or_default();
    let name = [
        row.base_url.to_string(),
        row.motis_version.to_string(),
        row.feed.as_ref().to_string(),
        row.country.clone().unwrap_or_default(),
        region,
    ]
    .into_iter()
    .chain(row.overture_release.map(|release| release.to_string()))
    .chain(row.label.as_ref().map(|label| label.as_str().to_string()))
    .collect::<Vec<_>>()
    .join("\n");
    Uuid::new_v5(&OLD_NAMESPACE, name.as_bytes()).to_string()
}

fn source_of(row: &MotisSourceRowV3) -> Result<MotisSource, Refused> {
    let id = || row.source_id.to_string();
    let overture_region = match (row.overture_region_id, &row.overture_region_name) {
        (Some(region_id), Some(name)) => Some(OvertureRegion {
            id: region_id,
            name: name.clone(),
        }),
        (None, None) => None,
        _ => return Err(Refused::Region { id: id() }),
    };
    let area = match (&row.country, row.overture_release) {
        (Some(country), Some(overture_release)) => Some(Area {
            country: country
                .parse()
                .map_err(|source| Refused::Country { id: id(), source })?,
            overture_region,
            overture_release,
        }),
        (None, None) => None,
        _ => return Err(Refused::Area { id: id() }),
    };
    Ok(MotisSource {
        base_url: url::Url::parse(&row.base_url.to_string())
            .map_err(|source| Refused::BaseUrl { id: id(), source })?,
        motis_version: row.motis_version.clone(),
        feed: row.feed,
        area,
        label: row.label.clone(),
    })
}

fn migrated(row: &MotisSourceRowV3) -> Result<MotisSourceRowV4, Refused> {
    let computed = old_id(row);
    let held = row.source_id.to_string();
    if computed != held {
        return Err(Refused::OldId { held, computed });
    }
    Ok(MotisSourceRowV4 {
        source_id: source_of(row)?.id(),
        base_url: row.base_url.clone(),
        motis_version: row.motis_version.clone(),
        feed: row.feed,
        country: row.country.clone(),
        overture_region_id: row.overture_region_id,
        overture_region_name: row.overture_region_name.clone(),
        overture_release: row.overture_release,
        label: row.label.clone(),
        registered_at: row.registered_at,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "motis_migrate_source_v4=info".into()),
        )
        .init();

    let root = Args::parse().medallion.root()?;
    let existing = recorded(&root).await?;
    if !existing.is_empty() {
        return Err(Refused::AlreadyMigrated {
            rows: existing.len(),
        }
        .into());
    }

    let query = Query::new(root.clone());
    query.register_by_name(MOTIS_SOURCE_V3).await?;
    let old: Vec<MotisSourceRowV3> = query
        .rows(&format!("SELECT * FROM {}", MOTIS_SOURCE_V3.name))
        .await?;
    let new = old.iter().map(migrated).collect::<Result<Vec<_>, _>>()?;

    let written = root
        .rows_of::<MotisSourceRowV4>()
        .append_rows(Utc::now(), &new)
        .await?;
    for (old, new) in old.iter().zip(&new) {
        tracing::info!(old = %old.source_id, new = %new.source_id, "migrated a source");
    }
    tracing::info!(rows = written, "wrote motis_source_v4");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn thuringia() -> MotisSourceRowV3 {
        serde_json::from_value(json!({
            "source_id": "dbc79993-2a04-51e0-a6f4-245ab1dfe468",
            "base_url": "https://api.transitous.org/",
            "motis_version": "v2.11.3",
            "feed": "transitous",
            "country": "DE",
            "overture_region_id": "e0ff5492-5758-4335-aabd-af1747925cd1",
            "overture_region_name": "Thüringen",
            "overture_release": "2026-09-23.1",
            "label": null,
            "registered_at": 0,
        }))
        .unwrap()
    }

    fn legacy() -> MotisSourceRowV3 {
        let mut row = thuringia();
        row.source_id =
            serde_json::from_value(json!("ba47e8c2-43db-51b1-ac4a-22aaef82535d")).unwrap();
        row.base_url = "http://127.0.0.1:8080/".parse().unwrap();
        row.motis_version = "v2.10.2".parse().unwrap();
        row.feed = medallion_model::Feed::Local;
        row.country = None;
        row.overture_region_id = None;
        row.overture_region_name = None;
        row.overture_release = None;
        row.label = Some("gtfs.de".parse().unwrap());
        row
    }

    #[test]
    fn a_row_takes_the_id_its_fields_give_under_the_new_namespace() {
        let row = migrated(&thuringia()).unwrap();

        assert_eq!(
            row.source_id.to_string(),
            "70045828-c168-5bb0-ba31-0a273498bf80"
        );
    }

    #[test]
    fn a_source_with_no_area_keeps_every_field_but_its_id() {
        let old = legacy();

        let new = migrated(&old).unwrap();

        assert_ne!(new.source_id, old.source_id);
        assert_eq!(new.label, old.label);
        assert_eq!(new.country, None);
        assert_eq!(new.overture_release, None);
        assert_eq!(new.registered_at, old.registered_at);
    }

    #[test]
    fn a_row_whose_id_does_not_match_its_fields_is_refused() {
        let mut row = thuringia();
        row.overture_region_name = None;
        row.overture_region_id = None;

        let err = migrated(&row).unwrap_err();

        assert!(matches!(err, Refused::OldId { .. }), "{err}");
    }
}
