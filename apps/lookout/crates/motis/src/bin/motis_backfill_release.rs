use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use clap::Parser;

use medallion::{MedallionArgs, Query, Root};
use medallion_model::{
    MOTIS_SEGMENT_V2, MOTIS_SOURCE, MotisCaptureRow, MotisSource, MotisSourceId, MotisSourceRow,
    OvertureRelease,
};
use motis::source::recorded;

const UNRELEASED_SOURCE: &str = "033a0af9-4189-5324-806b-1b1ec6cdd535";
const FIRST_DAY_ON_THE_NEWER_RELEASE: &str = "2026-10-12";
const OLDER_RELEASE: &str = "2026-07-22.0";
const NEWER_RELEASE: &str = "2026-09-23.1";
const NEWER_RELEASE_FIRST_SEEN: &str = "2026-10-12T21:04:39.520Z";

const CAPTURE_COLUMNS: &str = "source_id, captured_at, trip_id, route_name, train_number, \
    agency_id, agency_name, mode, route_color, from_stop_id, from_lat, from_lon, to_stop_id, \
    to_lat, to_lon, departure, arrival, scheduled_departure, scheduled_arrival, realtime, polyline";

#[derive(Parser)]
#[command(
    about = "Give the one Motis source recorded without an Overture release its release, \
    splitting it in two where the captures moved to a newer release"
)]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let root = args.medallion.root().expect("locate the medallion store");

    let unreleased = unreleased_source(&root).await;
    let older = with_release(&unreleased, OLDER_RELEASE);
    let newer = with_release(&unreleased, NEWER_RELEASE);
    let newer_at: DateTime<Utc> = NEWER_RELEASE_FIRST_SEEN.parse().expect("an instant");
    let switch: NaiveDate = FIRST_DAY_ON_THE_NEWER_RELEASE.parse().expect("a date");
    println!("older release {OLDER_RELEASE}: {}", older.id());
    println!("newer release {NEWER_RELEASE}: {}", newer.id());

    let captures = captures_by_instant(&root).await;
    let unexpected: Vec<&MotisSourceId> = captures
        .values()
        .flatten()
        .map(|row| &row.source_id)
        .filter(|id| id.to_string() != UNRELEASED_SOURCE)
        .collect();
    assert!(
        unexpected.is_empty(),
        "captures of other sources: {unexpected:?}"
    );

    let source_files = std::fs::read_dir(root.dataset(MOTIS_SOURCE).dir())
        .expect("list the source files")
        .map(|entry| entry.expect("a source file").path())
        .collect::<Vec<_>>();
    assert_eq!(source_files.len(), 1, "{source_files:?}");
    std::fs::remove_file(&source_files[0]).expect("remove the unreleased source");
    let sources = root.rows_of::<MotisSourceRow>();
    sources
        .append_rows(
            unreleased.registered_at,
            &[older.row(unreleased.registered_at)],
        )
        .await
        .expect("record the older release's source");
    sources
        .append_rows(newer_at, &[newer.row(newer_at)])
        .await
        .expect("record the newer release's source");

    for (captured_at, rows) in captures {
        let source = if captured_at.date_naive() < switch {
            &older
        } else {
            &newer
        };
        let partition = root
            .rows_of::<MotisCaptureRow>()
            .on_date(captured_at.date_naive())
            .expect("a dated partition");
        let file = partition.batch_file(captured_at);
        assert!(
            file.exists(),
            "{} is not where it was read from",
            file.display()
        );
        std::fs::remove_file(&file).expect("remove the capture file");
        let rows: Vec<MotisCaptureRow> = rows
            .into_iter()
            .map(|row| MotisCaptureRow {
                source_id: source.id(),
                ..row
            })
            .collect();
        partition
            .append_rows(captured_at, &rows)
            .await
            .expect("rewrite the capture file");
        println!("{} rows to {}: {}", rows.len(), source.id(), file.display());
    }

    read_back(&root).await;
}

async fn unreleased_source(root: &Root) -> MotisSourceRow {
    let query = Query::new(root.clone());
    query
        .register_by_name(MOTIS_SOURCE)
        .await
        .expect("register the sources");
    let mut rows: Vec<MotisSourceRow> = query
        .rows(&format!(
            "SELECT source_id, base_url, motis_version, feed, country, region_id, region_name,
                    CAST(NULL AS VARCHAR) AS overture_release, registered_at
             FROM {}",
            MOTIS_SOURCE.name
        ))
        .await
        .expect("read the sources");
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = rows.remove(0);
    assert_eq!(row.source_id.to_string(), UNRELEASED_SOURCE);
    row
}

fn with_release(row: &MotisSourceRow, release: &str) -> MotisSource {
    let release: OvertureRelease = release.parse().expect("an Overture release");
    MotisSource::try_from(MotisSourceRow {
        overture_release: Some(release.to_string()),
        ..row.clone()
    })
    .expect("the recorded source, with a release")
}

async fn captures_by_instant(root: &Root) -> BTreeMap<DateTime<Utc>, Vec<MotisCaptureRow>> {
    let query = Query::new(root.clone());
    query
        .register_by_name(MOTIS_SEGMENT_V2)
        .await
        .expect("register the captures");
    let rows: Vec<MotisCaptureRow> = query
        .rows(&format!(
            "SELECT {CAPTURE_COLUMNS} FROM {}",
            MOTIS_SEGMENT_V2.name
        ))
        .await
        .expect("read the captures");
    let mut by_instant: BTreeMap<DateTime<Utc>, Vec<MotisCaptureRow>> = BTreeMap::new();
    for row in rows {
        by_instant.entry(row.captured_at).or_default().push(row);
    }
    by_instant
}

async fn read_back(root: &Root) {
    for row in recorded(root).await.expect("read the sources back") {
        let source = MotisSource::try_from(row.clone()).expect("a valid source");
        assert_eq!(source.id(), row.source_id, "{row:?}");
        println!(
            "source {} release {:?} registered {}",
            row.source_id, row.overture_release, row.registered_at
        );
    }
    let query = Query::new(root.clone());
    query
        .register_by_name(MOTIS_SEGMENT_V2)
        .await
        .expect("register the captures");
    let per_source: Vec<PerSource> = query
        .rows(&format!(
            "SELECT source_id, COUNT(*) AS rows, COUNT(DISTINCT captured_at) AS polls
             FROM {} GROUP BY source_id ORDER BY source_id",
            MOTIS_SEGMENT_V2.name
        ))
        .await
        .expect("count the captures per source");
    for PerSource {
        source_id,
        rows,
        polls,
    } in per_source
    {
        println!("captures of {source_id}: {rows} rows over {polls} polls");
    }
}

#[derive(serde::Deserialize)]
struct PerSource {
    source_id: String,
    rows: i64,
    polls: i64,
}
