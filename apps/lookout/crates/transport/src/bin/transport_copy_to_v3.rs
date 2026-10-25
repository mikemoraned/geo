use std::error::Error;

use chrono::Utc;
use clap::Parser;
use medallion::{MedallionArgs, Query};
use medallion_model::{
    EXTRACT_MANIFEST_V2, EXTRACT_MANIFEST_V3, ExtractManifestRowV2, ExtractManifestRowV3,
    OvertureRelease,
};

#[derive(Parser)]
#[command(
    about = "Copy every row of extract_manifest_v2 into extract_manifest_v3, reading each \
    release as a checked type. Skip the copy if the third version holds rows already."
)]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let root = args.medallion.root()?;

    let query = Query::new(root.clone());
    query.register_by_name(EXTRACT_MANIFEST_V2).await?;
    let copied = query
        .rows::<ExtractManifestRowV2>(&in_order(EXTRACT_MANIFEST_V2.name))
        .await?
        .into_iter()
        .map(manifest_v3)
        .collect::<Result<Vec<_>, _>>()?;

    let third = root.rows_of::<ExtractManifestRowV3>();
    let written = match third.is_filled() {
        true => 0,
        false => third.append_rows(Utc::now(), &copied).await?,
    };

    query.register_by_name(EXTRACT_MANIFEST_V3).await?;
    let read_back: Vec<ExtractManifestRowV3> =
        query.rows(&in_order(EXTRACT_MANIFEST_V3.name)).await?;
    if read_back != copied {
        return Err("extract_manifest_v3 reads back other than extract_manifest_v2".into());
    }

    println!(
        "extract_manifest_v2 into extract_manifest_v3: {written} rows written, {} read back",
        read_back.len()
    );
    Ok(())
}

fn in_order(table: &str) -> String {
    format!("SELECT * FROM {table} ORDER BY extract_id")
}

fn manifest_v3(row: ExtractManifestRowV2) -> Result<ExtractManifestRowV3, Box<dyn Error>> {
    Ok(ExtractManifestRowV3 {
        extract_id: row.extract_id,
        extracted_at: row.extracted_at,
        overture_release: OvertureRelease::new(row.overture_release)?,
        country: row.country,
        min_lon: row.min_lon,
        min_lat: row.min_lat,
        max_lon: row.max_lon,
        max_lat: row.max_lat,
    })
}
