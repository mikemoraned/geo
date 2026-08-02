//! `extract`: take a point-in-time Overture extract into bronze — the country's rail,
//! water and administrative divisions from one release, under one extract id, with a
//! manifest row recording what was taken.
//!
//! Two modes, because filling in an extract already recorded and taking a new one are
//! different things: `backfill` re-fetches what a manifest row describes, under that
//! extract's own id, and `new` takes a fresh extract under a new one. Backfilling is the
//! default, since a store whose extracts are absent is the common case — they are 1.5 GB
//! and re-derivable, so they are not kept in version control while the manifest is.
//!
//! The release can be read from the public bucket or from a local mirror of it. That is a
//! choice of route, not of data: the same release read either way produces the same
//! extract, and the manifest records the release rather than the route.

use std::path::PathBuf;

use chrono::Utc;
use clap::{Parser, Subcommand};
use medallion::{Country, MedallionArgs, Root};
use model::ExtractManifestRow;
use transport::{
    extract::{self, ExtractId, Extraction, Extractor},
    overture::{DEFAULT_RELEASE, Overture, Release},
};

#[derive(Parser)]
#[command(about = "Extract Overture rail, water and divisions into bronze")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    medallion: MedallionArgs,
    /// Read the release from a local mirror rooted here, rather than from S3. The path
    /// holds the release's own `theme=…` directories.
    #[arg(long, global = true)]
    mirror: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Take a recorded extract again, from the release and window its manifest row
    /// records, under the id it was taken under. Defaults to the newest extract recorded.
    Backfill {
        /// The extract to take again, e.g. `20260727T193628Z`.
        extract_id: Option<ExtractId>,
    },
    /// Take a new extract, under a new id, and record it in the manifest.
    New {
        /// Overture release to extract (see docs.overturemaps.org/release).
        #[arg(long, default_value = DEFAULT_RELEASE)]
        release: String,
        /// Country to extract, as an ISO 3166-1 alpha-2 code.
        #[arg(long, default_value = "DE")]
        country: Country,
    },
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
    let root = args.medallion.root()?;
    let at = Utc::now();

    let extraction = match args
        .command
        .unwrap_or(Command::Backfill { extract_id: None })
    {
        Command::Backfill { extract_id } => {
            let recorded = match &extract_id {
                Some(id) => extract::recorded_as(&root, id).await?,
                None => extract::newest(&root).await?,
            };
            backfill(&root, &args.mirror, recorded, at).await?
        }
        Command::New { release, country } => {
            let id = ExtractId::at(at);
            let overture = Overture::open(release_at(&release, &args.mirror));
            tracing::info!(
                %id,
                %release,
                %country,
                root = %root.path().display(),
                "extracting",
            );
            Extractor::new(&overture, &root)
                .extract(id, country, at)
                .await?
        }
    };

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

async fn backfill(
    root: &Root,
    mirror: &Option<PathBuf>,
    recorded: ExtractManifestRow,
    at: chrono::DateTime<Utc>,
) -> Result<Extraction, Box<dyn std::error::Error>> {
    let overture = Overture::open(release_at(&recorded.release, mirror));
    tracing::info!(
        id = %recorded.extract_id,
        release = %recorded.release,
        country = %recorded.country,
        extracted_at = %recorded.extracted_at,
        root = %root.path().display(),
        "backfilling",
    );
    Ok(Extractor::new(&overture, root)
        .backfill(&recorded, at)
        .await?)
}

/// The release read from wherever it was asked for: a local mirror, or the public bucket.
fn release_at(release: &str, mirror: &Option<PathBuf>) -> Release {
    match mirror {
        Some(path) => Release::mirrored(release, path.clone()),
        None => Release::published(release),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Named nothing, this fills in what the store is missing rather than taking a new
    /// extract: the store's extracts are absent far more often than they are out of date.
    #[test]
    fn taking_the_newest_recorded_extract_again_is_the_default() {
        let args = Args::parse_from(["extract"]);

        assert!(matches!(
            args.command
                .unwrap_or(Command::Backfill { extract_id: None }),
            Command::Backfill { extract_id: None }
        ));
    }

    #[test]
    fn an_extract_to_take_again_is_named_by_its_id() {
        let args = Args::parse_from(["extract", "backfill", "20260727T193628Z"]);

        let Some(Command::Backfill {
            extract_id: Some(id),
        }) = args.command
        else {
            panic!("expected a backfill of a named extract");
        };
        assert_eq!(id.to_string(), "20260727T193628Z");
    }

    #[test]
    fn a_new_extract_defaults_to_the_pinned_release_and_germany() {
        let args = Args::parse_from(["extract", "new"]);

        let Some(Command::New { release, country }) = args.command else {
            panic!("expected a new extract");
        };
        assert_eq!(release, DEFAULT_RELEASE);
        assert_eq!(country, Country::Germany);
    }
}
