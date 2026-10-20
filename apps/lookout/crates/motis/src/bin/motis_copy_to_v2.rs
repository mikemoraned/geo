use std::collections::HashSet;

use chrono::Utc;
use clap::Parser;

use medallion::{MedallionArgs, Query, Root, Row};
use medallion_model::{
    ExtractManifestRow, ExtractManifestRowV2, MotisSourceId, MotisSourceRow, MotisSourceRowV2,
};
use motis::source::recorded;

#[derive(Parser)]
#[command(
    about = "Copy every row of motis_source and extract_manifest into its second version, \
    skipping a row the second version already holds"
)]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let root = args.medallion.root().expect("locate the medallion store");
    let at = Utc::now();

    let held_sources: HashSet<MotisSourceId> = held::<MotisSourceRowV2>(&root)
        .await
        .into_iter()
        .map(|row| row.source_id)
        .collect();
    let sources: Vec<MotisSourceRowV2> = recorded(&root)
        .await
        .expect("read motis_source")
        .into_iter()
        .filter(|row| !held_sources.contains(&row.source_id))
        .map(source_v2)
        .collect();
    let copied = root
        .rows_of::<MotisSourceRowV2>()
        .append_rows(at, &sources)
        .await
        .expect("write motis_source_v2");
    println!(
        "motis_source_v2: {copied} copied, {} already held",
        held_sources.len()
    );

    let held_extracts: HashSet<String> = held::<ExtractManifestRowV2>(&root)
        .await
        .into_iter()
        .map(|row| row.extract_id)
        .collect();
    let extracts: Vec<ExtractManifestRowV2> = transport::extract::recorded(&root)
        .await
        .expect("read extract_manifest")
        .into_iter()
        .filter(|row| !held_extracts.contains(&row.extract_id))
        .map(manifest_v2)
        .collect();
    let copied = root
        .rows_of::<ExtractManifestRowV2>()
        .append_rows(at, &extracts)
        .await
        .expect("write extract_manifest_v2");
    println!(
        "extract_manifest_v2: {copied} copied, {} already held",
        held_extracts.len()
    );

    read_back(&root).await;
}

fn source_v2(row: MotisSourceRow) -> MotisSourceRowV2 {
    MotisSourceRowV2 {
        source_id: row.source_id,
        base_url: row.base_url,
        motis_version: row.motis_version,
        feed: row.feed,
        country: row.country,
        overture_region_id: row.region_id,
        overture_region_name: row.region_name,
        overture_release: row.overture_release,
        label: row.label,
        registered_at: row.registered_at,
    }
}

fn manifest_v2(row: ExtractManifestRow) -> ExtractManifestRowV2 {
    ExtractManifestRowV2 {
        extract_id: row.extract_id,
        extracted_at: row.extracted_at,
        overture_release: row.release,
        country: row.country,
        min_lon: row.min_lon,
        min_lat: row.min_lat,
        max_lon: row.max_lon,
        max_lat: row.max_lat,
    }
}

async fn held<T>(root: &Root) -> Vec<T>
where
    T: Row + for<'de> serde::Deserialize<'de>,
{
    let query = Query::new(root.clone());
    let present = query
        .register_if_present(T::DATASET, T::DATASET.name)
        .await
        .expect("register the second version");
    if !present {
        return Vec::new();
    }
    query
        .rows(&format!("SELECT * FROM {}", T::DATASET.name))
        .await
        .expect("read the second version")
}

async fn read_back(root: &Root) {
    let sources = held::<MotisSourceRowV2>(root).await;
    let missing: Vec<MotisSourceRowV2> = recorded(root)
        .await
        .expect("read motis_source")
        .into_iter()
        .map(source_v2)
        .filter(|row| !sources.contains(row))
        .collect();
    assert!(
        missing.is_empty(),
        "missing from motis_source_v2: {missing:?}"
    );

    let extracts = held::<ExtractManifestRowV2>(root).await;
    let missing: Vec<ExtractManifestRowV2> = transport::extract::recorded(root)
        .await
        .expect("read extract_manifest")
        .into_iter()
        .map(manifest_v2)
        .filter(|row| !extracts.contains(row))
        .collect();
    assert!(
        missing.is_empty(),
        "missing from extract_manifest_v2: {missing:?}"
    );

    println!(
        "every row of motis_source and extract_manifest reads back from its second version: \
         {} sources and {} extracts",
        sources.len(),
        extracts.len()
    );
}
