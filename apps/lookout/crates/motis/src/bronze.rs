//! The bronze capture log: every `TripSegment` a poll returns, written verbatim as one
//! parquet file per poll.
//!
//! Bronze is immutable and sample-shaped (see `docs/medallion.md`), so a poll never
//! rewrites an earlier file: it lands a new one under its own `polled_date`, named for the
//! instant of the poll. Duplication across overlapping polls is intentional — the same
//! scheduled leg re-seen is a fresh row — and dedup happens downstream in silver.
//!
//! Times are kept as instants and the polyline as the Google-encoded string the service
//! sent, since bronze records what arrived rather than a normalised form of it.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use medallion::{Layer, Root};
use motis_openapi_progenitor::types::TripSegment;
use serde::{Deserialize, Serialize};
use serde_arrow::schema::{SchemaLike, TracingOptions};
use serde_json::json;

use crate::client::TripDetails;

/// The bronze dataset polls append to.
const DATASET: &str = "motis_segment";

/// The columns holding an instant. They travel through serde as epoch milliseconds (an
/// `i64` carries no timezone or unit of its own), so each is declared here as a UTC
/// millisecond timestamp rather than landing in the file as a bare integer.
const INSTANT_COLUMNS: [&str; 5] = [
    "captured_at",
    "departure",
    "arrival",
    "scheduled_departure",
    "scheduled_arrival",
];

/// The arrow schema of a [`SegmentRow`], with the instant columns typed.
fn segment_fields() -> Result<Vec<arrow::datatypes::FieldRef>, BronzeError> {
    let options =
        INSTANT_COLUMNS
            .iter()
            .try_fold(TracingOptions::default(), |options, &name| {
                options.overwrite(
                    name,
                    json!({"name": name, "data_type": "Timestamp(Millisecond, Some(\"UTC\"))"}),
                )
            })?;
    Ok(Vec::<arrow::datatypes::FieldRef>::from_type::<SegmentRow>(
        options,
    )?)
}

/// Failure appending to the bronze capture log.
#[derive(Debug, thiserror::Error)]
pub enum BronzeError {
    #[error("building the record batch: {0}")]
    Encode(#[from] serde_arrow::Error),
    #[error("partitioning the capture log: {0}")]
    Path(#[from] medallion::PathError),
    #[error("writing the capture log: {0}")]
    Write(#[from] medallion::WriteError),
}

/// One polled segment, flattened: the trip it belongs to, its resolved agency and train
/// number, its endpoints, its realtime-corrected and scheduled times, and its geometry as
/// the encoded polyline.
#[derive(Debug, Serialize, Deserialize)]
struct SegmentRow {
    #[serde(with = "chrono::serde::ts_milliseconds")]
    captured_at: DateTime<Utc>,
    trip_id: String,
    route_name: Option<String>,
    train_number: Option<u32>,
    agency_id: Option<String>,
    agency_name: Option<String>,
    mode: String,
    route_color: Option<String>,
    from_stop_id: Option<String>,
    from_lat: f64,
    from_lon: f64,
    to_stop_id: Option<String>,
    to_lat: f64,
    to_lon: f64,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    departure: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    arrival: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    scheduled_departure: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    scheduled_arrival: DateTime<Utc>,
    realtime: bool,
    polyline: String,
}

impl SegmentRow {
    fn from_segment(
        captured_at: DateTime<Utc>,
        segment: &TripSegment,
        details: &HashMap<String, TripDetails>,
    ) -> Self {
        let trip = segment.trips.first();
        let trip_id = trip.map(|t| t.trip_id.as_str()).unwrap_or_default();
        let details = details.get(trip_id);
        let agency = details.map(|d| &d.agency);

        Self {
            captured_at,
            trip_id: trip_id.to_string(),
            route_name: trip.and_then(|t| {
                t.display_name
                    .clone()
                    .or_else(|| t.route_short_name.clone())
            }),
            train_number: details.and_then(|d| d.train_number.map(|n| n.get())),
            agency_id: agency.and_then(|a| a.id.clone()),
            agency_name: agency.and_then(|a| a.name.clone()),
            mode: segment.mode.to_string(),
            route_color: segment.route_color.clone(),
            from_stop_id: segment.from.stop_id.clone(),
            from_lat: segment.from.lat,
            from_lon: segment.from.lon,
            to_stop_id: segment.to.stop_id.clone(),
            to_lat: segment.to.lat,
            to_lon: segment.to.lon,
            departure: segment.departure,
            arrival: segment.arrival,
            scheduled_departure: segment.scheduled_departure,
            scheduled_arrival: segment.scheduled_arrival,
            realtime: segment.real_time,
            polyline: segment.polyline.clone(),
        }
    }
}

/// A handle on the bronze capture log within a medallion store.
#[derive(Debug, Clone)]
pub struct SegmentLog {
    root: Root,
}

impl SegmentLog {
    pub fn new(root: Root) -> Self {
        Self { root }
    }

    /// The file one poll at `captured_at` writes: `polled_date` partition, named for the
    /// instant of the poll.
    pub fn poll_file(&self, captured_at: DateTime<Utc>) -> Result<std::path::PathBuf, BronzeError> {
        Ok(self
            .root
            .dataset(Layer::Bronze, DATASET)
            .date_partition("polled_date", captured_at.date_naive())?
            .batch_file(captured_at))
    }

    /// Write one poll's `segments` as a single parquet file, returning how many rows
    /// landed. An empty poll writes nothing, so no empty files accumulate.
    pub async fn append(
        &self,
        captured_at: DateTime<Utc>,
        segments: &[TripSegment],
        details: &HashMap<String, TripDetails>,
    ) -> Result<usize, BronzeError> {
        if segments.is_empty() {
            return Ok(0);
        }
        let rows: Vec<SegmentRow> = segments
            .iter()
            .map(|segment| SegmentRow::from_segment(captured_at, segment, details))
            .collect();

        let batch = serde_arrow::to_record_batch(&segment_fields()?, &rows)?;

        medallion::write_batches(&self.poll_file(captured_at)?, &[batch]).await?;
        Ok(rows.len())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    use super::*;

    /// The captured real 4-segment, mode-varied fixture (rail/subway/tram/bus).
    fn fixture_segments() -> Vec<TripSegment> {
        serde_json::from_str(include_str!("../tests/fixtures/trips.json"))
            .expect("parse trips fixture")
    }

    fn read_back(path: &std::path::Path) -> arrow::array::RecordBatch {
        let file = std::fs::File::open(path).expect("open written file");
        let mut reader = ParquetRecordBatchReaderBuilder::try_new(file)
            .expect("parquet")
            .build()
            .expect("reader");
        reader.next().expect("a batch").expect("batch reads")
    }

    #[tokio::test]
    async fn a_poll_lands_one_file_named_for_its_instant_under_its_polled_date() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = SegmentLog::new(Root::new(tmp.path()));
        let captured_at = Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 30).unwrap();

        let written = log
            .append(captured_at, &fixture_segments(), &HashMap::new())
            .await
            .expect("append");

        assert_eq!(written, fixture_segments().len());
        let path = log.poll_file(captured_at).expect("path");
        assert!(
            path.ends_with("bronze/motis_segment/polled_date=2026-07-26/20260726T140530Z.parquet")
        );
        assert_eq!(read_back(&path).num_rows(), fixture_segments().len());
    }

    #[tokio::test]
    async fn the_polyline_is_stored_verbatim() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = SegmentLog::new(Root::new(tmp.path()));
        // Whole milliseconds: instants are stored at millisecond precision.
        let captured_at = Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 30).unwrap();
        let segments = fixture_segments();

        log.append(captured_at, &segments, &HashMap::new())
            .await
            .expect("append");

        let batch = read_back(&log.poll_file(captured_at).expect("path"));
        let rows: Vec<SegmentRow> = serde_arrow::from_record_batch(&batch).expect("read rows");
        assert_eq!(
            rows.iter().map(|r| &r.polyline).collect::<Vec<_>>(),
            segments.iter().map(|s| &s.polyline).collect::<Vec<_>>(),
            "the encoded polylines should survive unchanged"
        );
        assert_eq!(
            rows[0].captured_at, captured_at,
            "the poll instant should round-trip"
        );
    }

    /// Two polls in the same second would collide on one filename; different instants get
    /// their own files, so neither poll's capture is lost.
    #[tokio::test]
    async fn separate_polls_write_separate_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = SegmentLog::new(Root::new(tmp.path()));
        let first = Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 30).unwrap();
        let second = Utc.with_ymd_and_hms(2026, 7, 26, 14, 6, 0).unwrap();

        log.append(first, &fixture_segments(), &HashMap::new())
            .await
            .expect("first");
        log.append(second, &fixture_segments(), &HashMap::new())
            .await
            .expect("second");

        let dir = log.poll_file(first).expect("path");
        let files = std::fs::read_dir(dir.parent().expect("partition dir"))
            .expect("read dir")
            .count();
        assert_eq!(files, 2);
    }

    #[tokio::test]
    async fn an_empty_poll_writes_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = SegmentLog::new(Root::new(tmp.path()));
        let captured_at = Utc::now();

        let written = log
            .append(captured_at, &[], &HashMap::new())
            .await
            .expect("append");

        assert_eq!(written, 0);
        assert!(!log.poll_file(captured_at).expect("path").exists());
    }
}
