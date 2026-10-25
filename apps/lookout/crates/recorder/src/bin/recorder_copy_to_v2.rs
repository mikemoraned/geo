use std::error::Error;

use chrono::{DateTime, NaiveDate, Utc};
use clap::Parser;
use medallion::{MedallionArgs, Query, Root, Row, fields, layers};
use medallion_model::{
    AccelReadingRow, AccelReadingRowV2, DeviceSessionRow, DeviceSessionRowV2, GpsReadingRow,
    GpsReadingRowV2,
};
use serde::Deserialize;

#[derive(Parser)]
#[command(
    about = "Copy every row of gps_reading, accel_reading, and device_session into that \
    dataset's second version, under the same ingestion date. Skip a date the second version \
    already holds."
)]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[derive(Deserialize)]
struct Ingested {
    ingested_date: NaiveDate,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let root = args.medallion.root()?;
    let at = Utc::now();

    copy(&root, at, gps_reading_v2).await?;
    copy(&root, at, accel_reading_v2).await?;
    copy(&root, at, device_session_v2).await?;
    Ok(())
}

fn gps_reading_v2(row: GpsReadingRow) -> GpsReadingRowV2 {
    GpsReadingRowV2 {
        device_id: row.device_id,
        t: row.t,
        lat: row.lat,
        lon: row.lon,
        alt: row.alt,
        acc: row.acc,
        speed: row.speed,
        heading: row.heading,
    }
}

fn accel_reading_v2(row: AccelReadingRow) -> AccelReadingRowV2 {
    AccelReadingRowV2 {
        device_id: row.device_id,
        t: row.t,
        rms: row.rms,
        peak: row.peak,
        n: row.n,
        x: row.x,
        y: row.y,
        z: row.z,
    }
}

fn device_session_v2(row: DeviceSessionRow) -> DeviceSessionRowV2 {
    DeviceSessionRowV2 {
        device_id: row.device_id,
        t: row.t,
        device_type: row.device_type,
        platform: row.platform,
        user_agent: row.user_agent,
        os: row.os,
        os_version: row.os_version,
    }
}

async fn copy<Old, New>(
    root: &Root,
    at: DateTime<Utc>,
    to_v2: fn(Old) -> New,
) -> Result<(), Box<dyn Error>>
where
    Old: Row<Layer = layers::Bronze>,
    New: Row<Layer = layers::Bronze> + PartialEq,
{
    let query = Query::new(root.clone());
    query
        .register_bronze(Old::DATASET, Old::DATASET.name)
        .await?;

    let (mut written, mut held) = (0, 0);
    let mut copied = Vec::new();
    for Ingested { ingested_date } in dates::<Old>(&query).await? {
        let rows: Vec<New> = rows_on::<Old>(&query, ingested_date)
            .await?
            .into_iter()
            .map(to_v2)
            .collect();
        let partition = root.rows_of::<New>().on_date(ingested_date)?;
        if partition.is_filled() {
            held += 1;
        } else {
            partition.append_rows(at, &rows).await?;
            written += 1;
        }
        copied.push((ingested_date, rows));
    }

    query
        .register_bronze(New::DATASET, New::DATASET.name)
        .await?;
    for (ingested_date, rows) in &copied {
        if rows_on::<New>(&query, *ingested_date).await? != *rows {
            return Err(format!(
                "{} on {ingested_date} reads back other than {}",
                New::DATASET.name,
                Old::DATASET.name
            )
            .into());
        }
    }

    println!(
        "{} into {}: {written} dates written and {held} already held, {} rows read back",
        Old::DATASET.name,
        New::DATASET.name,
        copied.iter().map(|(_, rows)| rows.len()).sum::<usize>()
    );
    Ok(())
}

async fn dates<T: Row>(query: &Query) -> Result<Vec<Ingested>, Box<dyn Error>> {
    let key = partition_key::<T>()?;
    Ok(query
        .rows(&format!(
            "SELECT DISTINCT {key} AS ingested_date FROM {name} ORDER BY ingested_date",
            name = T::DATASET.name
        ))
        .await?)
}

async fn rows_on<T: Row>(query: &Query, date: NaiveDate) -> Result<Vec<T>, Box<dyn Error>> {
    let key = partition_key::<T>()?;
    let fields = fields::<T>()?;
    let columns = fields
        .iter()
        .map(|field| field.name().as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Ok(query
        .rows(&format!(
            "SELECT {columns} FROM {name} WHERE {key} = '{date}' ORDER BY {columns}",
            name = T::DATASET.name
        ))
        .await?)
}

fn partition_key<T: Row>() -> Result<&'static str, Box<dyn Error>> {
    T::DATASET
        .partition_key
        .ok_or_else(|| format!("{} has no partition key", T::DATASET.name).into())
}
