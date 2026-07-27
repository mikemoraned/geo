//! `extract`: take a point-in-time Overture extract into bronze — the country's rail,
//! water and administrative divisions from one release, under one extract id, with a
//! manifest row recording what was taken.
//!
//! The release can be read from the public bucket or from a local mirror of it. That is a
//! choice of route, not of data: the same release read either way produces the same
//! extract, and the manifest records the release rather than the route.

use std::path::PathBuf;

use chrono::Utc;
use clap::Parser;
use medallion::{Country, MedallionArgs};
use transport::{
    extract::{ExtractId, Extractor},
    overture::{Overture, Release, DEFAULT_RELEASE},
};

#[derive(Parser)]
#[command(about = "Extract Overture rail, water and divisions into bronze")]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
    /// Overture release to extract (see docs.overturemaps.org/release).
    #[arg(long, default_value = DEFAULT_RELEASE)]
    release: String,
    /// Read the release from a local mirror rooted here, rather than from S3. The path
    /// holds the release's own `theme=…` directories.
    #[arg(long)]
    mirror: Option<PathBuf>,
    /// Country to extract, as an ISO 3166-1 alpha-2 code.
    #[arg(long, default_value = "DE")]
    country: Country,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "extract=info,transport=info".into()),
        )
        .init();

    let args = Args::parse();
    let release = match args.mirror {
        Some(path) => Release::mirrored(&args.release, path),
        None => Release::published(&args.release),
    };

    let at = Utc::now();
    let id = ExtractId::at(at);
    let root = args.medallion.root();
    let overture = Overture::open(release);

    tracing::info!(
        %id,
        release = %args.release,
        country = %args.country,
        root = %root.path().display(),
        "extracting",
    );

    let extraction = Extractor::new(&overture, &root)
        .extract(id, args.country, at)
        .await?;

    tracing::info!(
        id = %extraction.id,
        min_lon = extraction.window.min().x,
        min_lat = extraction.window.min().y,
        max_lon = extraction.window.max().x,
        max_lat = extraction.window.max().y,
        rows = extraction.rows.iter().map(|(_, rows)| rows).sum::<usize>(),
        "extracted",
    );
    Ok(())
}
