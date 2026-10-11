use chrono::{DateTime, Utc};
use clap::Parser;
use redis::aio::MultiplexedConnection;
use tracing_subscriber::EnvFilter;

use motis::capture::Coverage;
use motis::near_gps::{NearGps, Seen};
use motis::polling::{Look, PollArgs, Poller};
use motis::span::Span;

#[derive(Parser)]
#[command(about = "Poll Motis for train trips near recently logged GPS, and log them")]
struct Args {
    #[command(flatten)]
    poll: PollArgs,
    /// How long a GPS position stays in the rolling window before it is pruned, such as 30m.
    #[arg(long, default_value = "30m")]
    window_age: Span,
    /// Only ingest GPS samples captured within this span of now, such as 5m.
    #[arg(long, default_value = "5m")]
    recent_lookback: Span,
}

struct FollowGps {
    near_gps: NearGps,
    conn: MultiplexedConnection,
}

impl Look for FollowGps {
    async fn coverage(&mut self, now: DateTime<Utc>) -> Option<Coverage> {
        let Seen {
            ingested,
            positions,
            area,
        } = match self.near_gps.look(now, &mut self.conn).await {
            Ok(seen) => seen,
            Err(err) => {
                tracing::error!(%err, "reading recent GPS failed");
                return None;
            }
        };
        let Some(area) = area else {
            tracing::info!(ingested, "no recent gps positions; skipping motis query");
            return None;
        };
        tracing::info!(ingested, positions, "recent GPS names an area");
        Some(Coverage::bbox(area))
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "motis_follow_gps=info,motis=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args
        .poll
        .medallion
        .root()
        .expect("locate the medallion store");

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let url = std::env::var("LOOKOUT_REDIS_URL")
        .expect("LOOKOUT_REDIS_URL must be set — run via `just bronze-poll-motis`");
    let conn = telemetry::connect(&url)
        .await
        .expect("connect to telemetry redis");
    tracing::info!(
        window_age = %args.window_age,
        recent_lookback = %args.recent_lookback,
        "capturing near recent GPS"
    );
    let mut follow = FollowGps {
        near_gps: NearGps::new(args.window_age.age(), args.recent_lookback.age()),
        conn,
    };
    Poller::start(&args.poll, &root, None)
        .await
        .expect("start polling Motis")
        .run(&mut follow)
        .await;
}
