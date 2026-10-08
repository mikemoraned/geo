use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use geo_types::Rect;
use medallion_model::TripId;

use crate::api::types::{Mode, TripSegment};
use crate::bronze::{BronzeError, SegmentLog};
use crate::client::{MotisClient, MotisError};
use crate::details::TripDetailsCache;

#[derive(Debug)]
pub struct Capture {
    client: MotisClient,
    details: TripDetailsCache,
    log: SegmentLog,
    zoom: f64,
    window_half: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub segments: usize,
    pub unresolved: Vec<Unresolved>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unresolved {
    Segment { reason: String },
    Details { trip_id: TripId, reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("querying motis: {0}")]
    Motis(#[from] MotisError),
    #[error("writing capture log: {0}")]
    Log(#[from] BronzeError),
}

impl Capture {
    pub fn new(client: MotisClient, log: SegmentLog, zoom: f64, window_half: Duration) -> Self {
        Self {
            client,
            details: TripDetailsCache::default(),
            log,
            zoom,
            window_half,
        }
    }

    pub async fn capture(
        &mut self,
        now: DateTime<Utc>,
        area: &Rect<f64>,
    ) -> Result<Captured, CaptureError> {
        let window = now - self.window_half..now + self.window_half;
        let mut unresolved = Vec::new();
        let mut captured: Vec<TripSegment> = Vec::new();
        let mut trip_ids: HashSet<TripId> = HashSet::new();
        for segment in self
            .client
            .trips_in_bbox(area, &window, self.zoom)
            .await?
            .into_iter()
            .filter(|segment| is_rail(&segment.mode))
        {
            match segment.trip_id() {
                Ok(trip_id) => {
                    trip_ids.insert(trip_id);
                    captured.push(segment);
                }
                Err(err) => unresolved.push(Unresolved::Segment {
                    reason: err.to_string(),
                }),
            }
        }

        let resolution = self.details.resolve(&self.client, &trip_ids).await;
        unresolved.extend(resolution.failed.into_iter().map(|(trip_id, err)| {
            Unresolved::Details {
                trip_id,
                reason: err.to_string(),
            }
        }));
        let segments = self.log.append(now, &captured, &resolution.details).await?;

        Ok(Captured {
            segments,
            unresolved,
        })
    }
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
