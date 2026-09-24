use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use motis_openapi_progenitor::types::{Mode, TripSegment};
use redis::aio::MultiplexedConnection;
use shared::{Message, V0Message, V1Message};
use telemetry::RawSample;

use crate::bronze::{BronzeError, SegmentLog};
use crate::client::{MotisClient, MotisError, TimeWindow, TripDetails};
use crate::window::{Position, PositionWindow};

#[derive(Debug, Clone)]
pub struct PollConfig {
    pub recent_lookback: Duration,
    pub query_window_half: Duration,
    pub zoom: f64,
    pub sample_limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    NoRecentGps {
        ingested: usize,
    },
    Queried {
        ingested: usize,
        positions: usize,
        segments: usize,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum PollError {
    #[error("reading telemetry queue: {0}")]
    Queue(#[from] telemetry::QueueError),
    #[error("querying motis: {0}")]
    Motis(#[from] MotisError),
    #[error("writing capture log: {0}")]
    Log(#[from] BronzeError),
}

pub async fn poll_once(
    now: DateTime<Utc>,
    conn: &mut MultiplexedConnection,
    client: &MotisClient,
    log: &SegmentLog,
    window: &mut PositionWindow,
    config: &PollConfig,
) -> Result<PollOutcome, PollError> {
    let now_ms = now.timestamp_millis();
    let cutoff = now_ms - config.recent_lookback.as_millis() as i64;

    let samples = telemetry::peek_newest_samples(conn, config.sample_limit).await?;
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
    let segments: Vec<TripSegment> = client
        .trips_in_bbox(&bbox, &query_window, config.zoom)
        .await?
        .into_iter()
        .filter(|s| is_rail(&s.mode))
        .collect();
    let details = resolve_details(client, &segments).await;
    let written = log.append(now, &segments, &details).await?;

    Ok(PollOutcome::Queried {
        ingested,
        positions: window.len(),
        segments: written,
    })
}

fn is_rail(mode: &Mode) -> bool {
    matches!(
        mode,
        Mode::HighspeedRail
            | Mode::LongDistance
            | Mode::NightRail
            | Mode::RegionalFastRail
            | Mode::RegionalRail
            | Mode::Rail
    )
}

async fn resolve_details(
    client: &MotisClient,
    segments: &[TripSegment],
) -> HashMap<String, TripDetails> {
    let trip_ids: std::collections::HashSet<&str> = segments
        .iter()
        .filter_map(|s| s.trips.first())
        .map(|t| t.trip_id.as_str())
        .collect();
    let mut details = HashMap::new();
    for trip_id in trip_ids {
        match client.trip_details(trip_id).await {
            Ok(d) => {
                details.insert(trip_id.to_string(), d);
            }
            Err(err) => tracing::warn!(%trip_id, %err, "resolving trip details failed"),
        }
    }
    details
}

fn sample_gps(raw: &RawSample) -> Option<(i64, f64, f64)> {
    match raw.parse().ok()? {
        Message::Version0(V0Message::Gps(r)) | Message::Version1(V1Message::Gps(r)) => {
            Some((r.t, r.gps.latitude(), r.gps.longitude()))
        }
        _ => None,
    }
}
