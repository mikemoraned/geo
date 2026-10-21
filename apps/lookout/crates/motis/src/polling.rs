use std::future::Future;
use std::time::Instant;

use chrono::{DateTime, Utc};
use url::Url;

use domain::Span;
use medallion::{MedallionArgs, Root};
use medallion_model::{Area, Feed, Label, MotisSource};

use crate::bronze::SegmentLog;
use crate::capture::{Capture, Captured, Coverage, Unresolved};
use crate::client::{DEFAULT_BASE_URL, MotisClient, MotisError};
use crate::source::{SourceError, register};
use crate::zoom::Zoom;

#[derive(Debug, clap::Args)]
pub struct PollArgs {
    /// How long to poll for before stopping, such as 1h or 1d.
    #[arg(long)]
    pub poll_duration: Span,
    /// Time between polls, such as 30s or 10m.
    #[arg(long, default_value = "30s")]
    pub poll_interval: Span,
    /// Motis zoom level (higher adds subway/tram/bus on top of long-distance rail).
    #[arg(long, default_value_t = Zoom::default())]
    pub zoom: Zoom,
    /// Base URL of the Motis server.
    #[arg(long, default_value = DEFAULT_BASE_URL)]
    pub motis_url: Url,
    /// Which feed the Motis server answers from.
    #[arg(long)]
    pub feed: Feed,
    /// Free text telling two setups of one server apart, such as the timetable build.
    #[arg(long)]
    pub label: Option<Label>,
    #[command(flatten)]
    pub medallion: MedallionArgs,
}

pub trait Look {
    fn coverage(&mut self, now: DateTime<Utc>) -> impl Future<Output = Option<Coverage>>;
}

impl Look for Coverage {
    async fn coverage(&mut self, _now: DateTime<Utc>) -> Option<Coverage> {
        Some(self.clone())
    }
}

#[derive(Debug)]
pub struct Poller {
    capture: Capture,
    interval: Span,
    duration: Span,
}

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("asking the Motis server for its version: {0}")]
    Version(#[source] MotisError),
    #[error("recording the Motis source: {0}")]
    Source(#[from] SourceError),
}

impl Poller {
    pub async fn start(
        args: &PollArgs,
        root: &Root,
        area: Option<Area>,
    ) -> Result<Self, StartError> {
        let client = MotisClient::new(&args.motis_url);
        let motis_version = client.motis_version().await.map_err(StartError::Version)?;
        let source = MotisSource {
            base_url: args.motis_url.clone(),
            motis_version,
            feed: args.feed,
            area,
            label: args.label.clone(),
        };
        let registration = register(root, &source, Utc::now()).await?;
        tracing::info!(source_id = %source.id(), ?registration, "recorded the Motis source");
        tracing::info!(
            motis_url = %args.motis_url,
            medallion_root = %root.path().display(),
            poll_interval = %args.poll_interval,
            poll_duration = %args.poll_duration,
            zoom = %args.zoom,
            "starting motis poll loop (Ctrl-C to stop early)"
        );
        Ok(Self {
            capture: Capture::new(
                client,
                SegmentLog::new(root.clone(), source.id()),
                args.zoom,
                args.poll_interval,
            ),
            interval: args.poll_interval,
            duration: args.poll_duration,
        })
    }

    pub async fn run(mut self, look: &mut impl Look) {
        let mut ticker = tokio::time::interval(self.interval.duration());
        let finished = tokio::time::sleep(self.duration.duration());
        tokio::pin!(finished);
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("interrupted; stopping");
                    break;
                }
                _ = &mut finished => {
                    tracing::info!(poll_duration = %self.duration, "poll duration reached; stopping");
                    break;
                }
                _ = ticker.tick() => {
                    poll(Utc::now(), look, &mut self.capture).await;
                }
            }
        }
    }
}

async fn poll(now: DateTime<Utc>, look: &mut impl Look, capture: &mut Capture) {
    let Some(coverage) = look.coverage(now).await else {
        return;
    };
    tracing::info!("polling motis");
    let started = Instant::now();
    let captured = capture.capture(now, &coverage).await;
    let took = started.elapsed();
    match captured {
        Ok(Captured {
            segments,
            outside,
            unresolved,
            requests,
            filter_took,
            log_took,
        }) => {
            tracing::info!(
                segments,
                outside,
                unresolved = unresolved.len(),
                ?took,
                map_trips_requests = requests.map_trips.requests,
                map_trips_took = ?requests.map_trips.took,
                trip_requests = requests.trip.requests,
                trip_took = ?requests.trip.took,
                ?filter_took,
                ?log_took,
                "polled motis"
            );
            for failure in &unresolved {
                match failure {
                    Unresolved::Segment { reason } => {
                        tracing::error!(%reason, "a segment was not captured");
                    }
                    Unresolved::Details { trip_id, reason } => {
                        tracing::error!(%trip_id, %reason, "a trip was captured without its details");
                    }
                }
            }
        }
        Err(err) => tracing::error!(%err, ?took, "capturing from motis failed"),
    }
}
