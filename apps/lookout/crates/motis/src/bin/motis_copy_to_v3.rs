use std::error::Error;

use chrono::Utc;
use clap::Parser;
use medallion::{MedallionArgs, Query};
use medallion_model::{
    GersId, MOTIS_SOURCE_V2, MOTIS_SOURCE_V3, MotisBaseUrl, MotisSourceRowV2, MotisSourceRowV3,
};

#[derive(Parser)]
#[command(
    about = "Copy every row of motis_source_v2 into motis_source_v3, reading each base URL and \
    region id as a checked type. Skip the copy if the third version holds rows already."
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
    query.register_by_name(MOTIS_SOURCE_V2).await?;
    let copied = query
        .rows::<MotisSourceRowV2>(&in_order(MOTIS_SOURCE_V2.name))
        .await?
        .into_iter()
        .map(source_v3)
        .collect::<Result<Vec<_>, _>>()?;

    let third = root.rows_of::<MotisSourceRowV3>();
    let written = match third.is_filled() {
        true => 0,
        false => third.append_rows(Utc::now(), &copied).await?,
    };

    query.register_by_name(MOTIS_SOURCE_V3).await?;
    let read_back: Vec<MotisSourceRowV3> = query.rows(&in_order(MOTIS_SOURCE_V3.name)).await?;
    if read_back != copied {
        return Err("motis_source_v3 reads back other than motis_source_v2".into());
    }

    println!(
        "motis_source_v2 into motis_source_v3: {written} rows written, {} read back",
        read_back.len()
    );
    Ok(())
}

fn in_order(table: &str) -> String {
    format!("SELECT * FROM {table} ORDER BY source_id")
}

fn source_v3(row: MotisSourceRowV2) -> Result<MotisSourceRowV3, Box<dyn Error>> {
    Ok(MotisSourceRowV3 {
        source_id: row.source_id,
        base_url: MotisBaseUrl::new(row.base_url)?,
        motis_version: row.motis_version,
        feed: row.feed,
        country: row.country,
        overture_region_id: row.overture_region_id.map(GersId::new).transpose()?,
        overture_region_name: row.overture_region_name,
        overture_release: row.overture_release,
        label: row.label,
        registered_at: row.registered_at,
    })
}
