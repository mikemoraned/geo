use chrono::{DateTime, Utc};
use clap::Parser;
use redis::aio::MultiplexedConnection;
use tracing_subscriber::EnvFilter;
use url::Url;

use medallion::{Country, MedallionArgs, Root};
use medallion_model::{Area, Feed, MotisSource, Region};
use motis::bronze::SegmentLog;
use motis::capture::{Capture, Captured, Coverage, Unresolved};
use motis::client::{DEFAULT_BASE_URL, MotisClient};
use motis::near_gps::{NearGps, Seen};
use motis::source::register;
use motis::span::Span;
use transport::countries::CountryAreas;
use transport::regions::{RegionCode, region_area};

const DEFAULT_ZOOM: f64 = 8.0;

const SAMPLE_LIMIT: usize = 1000;

#[derive(Parser)]
#[command(
    about = "Poll Motis for train trips, near recently logged GPS or in one Overture country or region, and log them"
)]
struct Args {
    /// How long to poll for before stopping, such as 1h or 1d.
    #[arg(long)]
    poll_duration: Span,
    /// Time between polls, such as 30s or 10m.
    #[arg(long, default_value = "30s")]
    poll_interval: Span,
    /// How long a GPS position stays in the rolling window before it is pruned, such as 30m.
    #[arg(long, default_value = "30m")]
    window_age: Span,
    /// Only ingest GPS samples captured within this span of now, such as 5m.
    #[arg(long, default_value = "5m")]
    recent_lookback: Span,
    /// Half the width of the window of departures each poll asks about, centred on now.
    #[arg(long, default_value = "5m")]
    query_window_half: Span,
    /// Motis zoom level (higher adds subway/tram/bus on top of long-distance rail).
    #[arg(long, default_value_t = DEFAULT_ZOOM)]
    zoom: f64,
    /// Base URL of the Motis server.
    #[arg(long, default_value = DEFAULT_BASE_URL)]
    motis_url: Url,
    /// Which feed the Motis server answers from.
    #[arg(long)]
    feed: Feed,
    /// Capture every train in this country, as Overture's `country` column names it, such as DE,
    /// instead of trains near recent GPS. Its shape comes from the country's newest extract.
    #[arg(long)]
    overture_country: Option<Country>,
    /// Narrow the country to this region, as Overture's `region` column names it, such as DE-TH.
    #[arg(long, requires = "overture_country")]
    overture_region: Option<RegionCode>,
    #[command(flatten)]
    medallion: MedallionArgs,
}

enum Look {
    NearGps {
        near_gps: NearGps,
        conn: MultiplexedConnection,
    },
    Fixed(Coverage),
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "motis_poll=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args.medallion.root().expect("locate the medallion store");

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let client = MotisClient::new(args.motis_url.as_str().trim_end_matches('/'));
    let motis_version = client
        .motis_version()
        .await
        .expect("ask the Motis server for its version");

    let (area, mut look) = match args.overture_country {
        Some(country) => {
            let (area, coverage) = fixed_area(&root, country, args.overture_region.as_ref()).await;
            (Some(area), Look::Fixed(coverage))
        }
        None => (None, near_gps(&args).await),
    };

    let source = MotisSource {
        base_url: args.motis_url.clone(),
        motis_version,
        feed: args.feed,
        area,
    };
    let registration = register(&root, &source, Utc::now())
        .await
        .expect("record the Motis source");
    tracing::info!(source_id = %source.id(), ?registration, "recorded the Motis source");
    let mut capture = Capture::new(
        client,
        SegmentLog::new(root.clone(), source.id()),
        args.zoom,
        args.query_window_half,
    );

    tracing::info!(
        motis_url = %args.motis_url,
        medallion_root = %root.path().display(),
        poll_interval = %args.poll_interval,
        poll_duration = %args.poll_duration,
        zoom = args.zoom,
        "starting motis poll loop (Ctrl-C to stop early)"
    );

    let mut ticker = tokio::time::interval(args.poll_interval.duration());
    let finished = tokio::time::sleep(args.poll_duration.duration());
    tokio::pin!(finished);
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("interrupted; stopping");
                break;
            }
            _ = &mut finished => {
                tracing::info!(poll_duration = %args.poll_duration, "poll duration reached; stopping");
                break;
            }
            _ = ticker.tick() => {
                poll(Utc::now(), &mut look, &mut capture).await;
            }
        }
    }
}

async fn fixed_area(
    root: &Root,
    country: Country,
    region: Option<&RegionCode>,
) -> (Area, Coverage) {
    let (region, shapes) = match region {
        Some(code) => {
            let found = region_area(root, country, code)
                .await
                .expect("find the region in the country's newest Overture extract");
            let region = Region {
                id: found.id,
                name: found.name,
            };
            (Some(region), found.shapes)
        }
        None => {
            let shapes = CountryAreas::newest_per_country(root)
                .await
                .expect("read the country areas from the Overture extracts")
                .shapes_of(country);
            (None, shapes)
        }
    };
    let coverage = Coverage::shapes(shapes)
        .expect("the newest Overture extract holds a shape for the area to capture");
    tracing::info!(
        country = country.code(),
        region = region.as_ref().map(|region| region.name.as_str()),
        "capturing a fixed area"
    );
    (Area { country, region }, coverage)
}

async fn near_gps(args: &Args) -> Look {
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
    Look::NearGps {
        near_gps: NearGps::new(
            args.window_age.age(),
            args.recent_lookback.age(),
            SAMPLE_LIMIT,
        ),
        conn,
    }
}

async fn poll(now: DateTime<Utc>, look: &mut Look, capture: &mut Capture) {
    let coverage = match look {
        Look::Fixed(coverage) => coverage.clone(),
        Look::NearGps { near_gps, conn } => {
            let Seen {
                ingested,
                positions,
                area,
            } = match near_gps.look(now, conn).await {
                Ok(seen) => seen,
                Err(err) => return tracing::error!(%err, "reading recent GPS failed"),
            };
            let Some(area) = area else {
                return tracing::info!(ingested, "no recent gps positions; skipping motis query");
            };
            tracing::info!(ingested, positions, "recent GPS names an area");
            Coverage::bbox(area)
        }
    };
    match capture.capture(now, &coverage).await {
        Ok(Captured {
            segments,
            outside,
            unresolved,
        }) => {
            tracing::info!(
                segments,
                outside,
                unresolved = unresolved.len(),
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
        Err(err) => tracing::error!(%err, "capturing from motis failed"),
    }
}
