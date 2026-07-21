//! The core of one poll tick, independent of the CLI: refresh a rolling GPS window from
//! the latest telemetry samples, then query Motis for trips in its buffered bbox and
//! append them to the capture log. The `motis_poll` binary is a thin loop around
//! [`poll_once`]; tests drive it directly against a real redis and a mock Motis server.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use motis_openapi_progenitor::types::TripSegment;
use redis::aio::MultiplexedConnection;
use shared::{Message, V0Message, V1Message};
use telemetry::RawSample;

use crate::client::{Agency, MotisClient, MotisError, TimeWindow};
use crate::store::{Store, StoreError};
use crate::window::{Position, PositionWindow};

/// Knobs for one poll tick.
#[derive(Debug, Clone)]
pub struct PollConfig {
    /// Only ingest GPS samples captured within this age of `now`.
    pub recent_lookback: Duration,
    /// Half-width of the `map/trips` time window queried around `now`.
    pub query_window_half: Duration,
    /// Motis zoom level (higher adds subway/tram/bus on top of long-distance rail).
    pub zoom: f64,
    /// How many of the most-recent queued samples to scan for GPS.
    pub sample_limit: usize,
}

/// The result of one [`poll_once`] tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    /// No GPS in the recent window, so the Motis query was skipped.
    NoRecentGps { ingested: usize },
    /// Queried Motis and appended `segments` rows to the capture log.
    Queried {
        ingested: usize,
        positions: usize,
        segments: usize,
    },
}

/// A failure during a poll tick.
#[derive(Debug, thiserror::Error)]
pub enum PollError {
    #[error("reading telemetry queue: {0}")]
    Queue(#[from] telemetry::QueueError),
    #[error("querying motis: {0}")]
    Motis(#[from] MotisError),
    #[error("writing capture log: {0}")]
    Store(#[from] StoreError),
}

/// One poll: ingest the latest recent GPS into `window`, then (if any) log the Motis
/// trips in its buffered bbox over a short window around `now`.
pub async fn poll_once(
    now: DateTime<Utc>,
    conn: &mut MultiplexedConnection,
    client: &MotisClient,
    store: &Store,
    window: &mut PositionWindow,
    config: &PollConfig,
) -> Result<PollOutcome, PollError> {
    let now_ms = now.timestamp_millis();
    let cutoff = now_ms - config.recent_lookback.as_millis() as i64;

    let samples = telemetry::latest_samples(conn, config.sample_limit).await?;
    let mut ingested = 0;
    for (t, lat, lon) in samples
        .iter()
        .filter_map(sample_gps)
        .filter(|(t, _, _)| *t >= cutoff)
    {
        window.ingest(Position { t, lat, lon });
        ingested += 1;
    }
    window.prune(now_ms);

    let Some(bbox) = window.buffered_bbox() else {
        return Ok(PollOutcome::NoRecentGps { ingested });
    };

    let half = chrono::Duration::from_std(config.query_window_half)
        .expect("query window fits in chrono::Duration");
    let query_window = TimeWindow::around(now, half);
    let segments = client.trips_in_bbox(&bbox, &query_window, config.zoom).await?;
    let agencies = resolve_agencies(client, &segments).await;
    let written = store.insert(now, &segments, &agencies)?;

    Ok(PollOutcome::Queried {
        ingested,
        positions: window.len(),
        segments: written,
    })
}

/// Resolve the operating [`Agency`] of each distinct `trip_id` in `segments` via the
/// Motis `trip` endpoint, keyed by `trip_id`. Stateless — no caching across ticks, since
/// Motis is local and the poll interval is coarse. A trip whose lookup fails or names no
/// agency is simply omitted (the store writes `NULL` for it); a failure is logged, never
/// fatal, so a resolve error can't drop the segment.
async fn resolve_agencies(
    client: &MotisClient,
    segments: &[TripSegment],
) -> HashMap<String, Agency> {
    let trip_ids: std::collections::HashSet<&str> = segments
        .iter()
        .filter_map(|s| s.trips.first())
        .map(|t| t.trip_id.as_str())
        .collect();
    let mut agencies = HashMap::new();
    for trip_id in trip_ids {
        match client.trip_agency(trip_id).await {
            Ok(Some(agency)) => {
                agencies.insert(trip_id.to_string(), agency);
            }
            Ok(None) => {}
            Err(err) => tracing::warn!(%trip_id, %err, "resolving trip agency failed"),
        }
    }
    agencies
}

/// The `(t, lat, lon)` of a sample if it is a GPS fix, else `None` (accel/session or an
/// unparseable payload are skipped).
fn sample_gps(raw: &RawSample) -> Option<(i64, f64, f64)> {
    match raw.parse().ok()? {
        Message::Version0(V0Message::Gps(r)) | Message::Version1(V1Message::Gps(r)) => {
            Some((r.t, r.gps.lat, r.gps.lon))
        }
        _ => None,
    }
}
