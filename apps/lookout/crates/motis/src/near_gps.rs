use chrono::{DateTime, Utc};
use domain::Age;
use geo_types::{Point, Rect};
use redis::aio::MultiplexedConnection;
use shared::{Message, V0Message, V1Message};
use telemetry::RawSample;

use crate::window::{Position, PositionWindow};

const SAMPLE_LIMIT: usize = 1000;

#[derive(Debug, Clone)]
pub struct NearGps {
    window: PositionWindow,
    lookback: Age,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    pub ingested: usize,
    pub positions: usize,
    pub area: Option<Rect<f64>>,
}

impl NearGps {
    pub fn new(window_age: Age, lookback: Age) -> Self {
        Self {
            window: PositionWindow::new(window_age),
            lookback,
        }
    }

    pub async fn look(
        &mut self,
        now: DateTime<Utc>,
        conn: &mut MultiplexedConnection,
    ) -> Result<Seen, telemetry::QueueError> {
        let cutoff = self.lookback.before(now);
        let samples = telemetry::peek_newest_samples(conn, SAMPLE_LIMIT).await?;
        let recent: Vec<Position> = samples
            .iter()
            .filter_map(position_of)
            .filter(|position| position.at >= cutoff)
            .collect();
        let ingested = recent.len();
        for position in recent {
            self.window.ingest(position);
        }
        self.window.prune(now);

        Ok(Seen {
            ingested,
            positions: self.window.len(),
            area: self.window.buffered_bbox(),
        })
    }
}

fn position_of(raw: &RawSample) -> Option<Position> {
    match raw.parse().ok()? {
        Message::Version0(V0Message::Gps(r)) | Message::Version1(V1Message::Gps(r)) => {
            Some(Position {
                at: DateTime::from_timestamp_millis(r.t)?,
                point: Point::new(r.gps.longitude(), r.gps.latitude()),
            })
        }
        _ => None,
    }
}
