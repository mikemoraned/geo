use std::collections::HashMap;

use chrono::{DateTime, Utc};
use medallion::Root;
use medallion_model::MotisSegmentRow;
use motis_openapi_progenitor::types::TripSegment;

use crate::client::TripDetails;

#[derive(Debug, thiserror::Error)]
pub enum BronzeError {
    #[error("partitioning the capture log: {0}")]
    Path(#[from] medallion::PathError),
    #[error("writing the capture log: {0}")]
    Write(#[from] medallion::AppendError),
}

fn segment_row(
    captured_at: DateTime<Utc>,
    segment: &TripSegment,
    details: &HashMap<String, TripDetails>,
) -> MotisSegmentRow {
    let trip = segment.trips.first();
    let trip_id = trip.map(|t| t.trip_id.as_str()).unwrap_or_default();
    let details = details.get(trip_id);
    let agency = details.map(|d| &d.agency);

    MotisSegmentRow {
        captured_at,
        trip_id: trip_id.to_string(),
        route_name: trip.and_then(|t| {
            t.display_name
                .clone()
                .or_else(|| t.route_short_name.clone())
        }),
        train_number: details.and_then(|d| d.train_number),
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

#[derive(Debug, Clone)]
pub struct SegmentLog {
    root: Root,
}

impl SegmentLog {
    pub fn new(root: Root) -> Self {
        Self { root }
    }

    fn partition(
        &self,
        captured_at: DateTime<Utc>,
    ) -> Result<medallion::Dataset<medallion::layers::Bronze>, BronzeError> {
        Ok(self
            .root
            .rows_of::<MotisSegmentRow>()
            .on_date(captured_at.date_naive())?)
    }

    pub fn poll_file(&self, captured_at: DateTime<Utc>) -> Result<std::path::PathBuf, BronzeError> {
        Ok(self.partition(captured_at)?.batch_file(captured_at))
    }

    pub async fn append(
        &self,
        captured_at: DateTime<Utc>,
        segments: &[TripSegment],
        details: &HashMap<String, TripDetails>,
    ) -> Result<usize, BronzeError> {
        let rows: Vec<MotisSegmentRow> = segments
            .iter()
            .map(|segment| segment_row(captured_at, segment, details))
            .collect();

        self.append_rows(captured_at, &rows).await
    }

    pub async fn append_rows(
        &self,
        captured_at: DateTime<Utc>,
        rows: &[MotisSegmentRow],
    ) -> Result<usize, BronzeError> {
        Ok(self
            .partition(captured_at)?
            .append_rows(captured_at, rows)
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    use super::*;

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
            path.ends_with(
                "bronze/motis_segment/polled_date=2026-07-26/20260726T140530000Z.parquet"
            )
        );
        assert_eq!(read_back(&path).num_rows(), fixture_segments().len());
    }

    #[tokio::test]
    async fn the_polyline_is_stored_verbatim() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = SegmentLog::new(Root::new(tmp.path()));
        let captured_at = Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 30).unwrap();
        let segments = fixture_segments();

        log.append(captured_at, &segments, &HashMap::new())
            .await
            .expect("append");

        let batch = read_back(&log.poll_file(captured_at).expect("path"));
        let rows: Vec<MotisSegmentRow> = serde_arrow::from_record_batch(&batch).expect("read rows");
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
