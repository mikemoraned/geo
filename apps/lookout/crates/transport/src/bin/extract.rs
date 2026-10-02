use std::path::PathBuf;
use std::process::ExitCode;

use chrono::Utc;
use clap::{Parser, Subcommand};
use medallion::{Country, MedallionArgs, Root};
use medallion_model::ExtractManifestRow;
use transport::{
    extract::{self, ExtractId, Extraction, Extractor},
    overture::{DEFAULT_RELEASE, Overture, Release},
};

#[derive(Parser)]
#[command(about = "Extract Overture rail, water, and divisions into bronze")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    medallion: MedallionArgs,
    /// Read the release from a local mirror of the bucket's `release/` prefix rooted
    /// here, rather than from S3. The path holds a directory per release, named by its
    /// id.
    #[arg(long, global = true)]
    mirror: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Take a recorded extract again, from the release and bbox its manifest row
    /// records, under the id it was taken under. With no id, takes every extract the
    /// manifest records, skipping the ones already filled in.
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
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "extract=info,transport=info".into()),
        )
        .init();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let root = args.medallion.root()?;
    let at = Utc::now();

    match args
        .command
        .unwrap_or(Command::Backfill { extract_id: None })
    {
        Command::Backfill { extract_id: None } => {
            let backfilled = backfill_recorded(&root, &args.mirror, at).await?;
            tracing::info!(
                filled = %named(&backfilled.filled),
                skipped = %named(&backfilled.skipped),
                "backfilled",
            );
        }
        Command::Backfill {
            extract_id: Some(id),
        } => {
            let recorded = extract::recorded_as(&root, &id).await?;
            report(&backfill(&root, &args.mirror, recorded, at).await?);
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
            report(
                &Extractor::new(&overture, &root)
                    .extract(id, country, at)
                    .await?,
            );
        }
    }

    Ok(())
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Backfilled {
    filled: Vec<ExtractId>,
    skipped: Vec<ExtractId>,
}

async fn backfill_recorded(
    root: &Root,
    mirror: &Option<PathBuf>,
    at: chrono::DateTime<Utc>,
) -> Result<Backfilled, Box<dyn std::error::Error>> {
    let recorded = extract::recorded(root).await?;
    if recorded.is_empty() {
        return Err(extract::ExtractError::NoExtract.into());
    }
    let in_the_order_taken: Vec<ExtractManifestRow> = recorded.into_iter().rev().collect();

    let mut skipped = Vec::new();
    let mut missing = Vec::new();
    for row in in_the_order_taken {
        let id = ExtractId::new(row.extract_id.clone())?;
        match extract::is_filled(root, &id)? {
            true => {
                tracing::info!(%id, country = %row.country, "already in the store");
                skipped.push(id);
            }
            false => missing.push(row),
        }
    }

    let mut filled = Vec::new();
    for row in missing {
        let extraction = backfill(root, mirror, row, at).await?;
        report(&extraction);
        filled.push(extraction.id);
    }

    Ok(Backfilled { filled, skipped })
}

fn named(ids: &[ExtractId]) -> String {
    match ids.is_empty() {
        true => "none".to_string(),
        false => ids
            .iter()
            .map(ExtractId::to_string)
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn report(extraction: &Extraction) {
    tracing::info!(
        id = %extraction.id,
        min_lon = extraction.bbox.min().x,
        min_lat = extraction.bbox.min().y,
        max_lon = extraction.bbox.max().x,
        max_lat = extraction.bbox.max().y,
        rows = extraction.rows.iter().map(|(_, rows)| rows).sum::<usize>(),
        "extracted",
    );
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

fn release_at(release: &str, mirror: &Option<PathBuf>) -> Release {
    match mirror {
        Some(path) => Release::mirrored(release, path.clone()),
        None => Release::published(release),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use medallion::Country;

    use super::*;

    fn recorded_row(id: &str, hour: u32) -> ExtractManifestRow {
        ExtractManifestRow {
            extract_id: id.to_string(),
            extracted_at: Utc.with_ymd_and_hms(2026, 9, 27, hour, 0, 0).unwrap(),
            release: "2026-07-22.0".to_string(),
            country: Country::Germany.code().to_string(),
            min_lon: 5.8,
            min_lat: 47.2,
            max_lon: 15.1,
            max_lat: 55.1,
        }
    }

    async fn store_filled_with(rows: &[ExtractManifestRow]) -> (tempfile::TempDir, Root) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        for row in rows {
            root.rows_of::<ExtractManifestRow>()
                .append_rows(row.extracted_at, std::slice::from_ref(row))
                .await
                .expect("record the extract");
            let extract = root
                .dataset(medallion_model::OVERTURE_EXTRACT)
                .for_id(&row.extract_id)
                .expect("an extract id")
                .partition("theme", "divisions")
                .expect("the divisions theme");
            std::fs::create_dir_all(extract.dir()).expect("the extract dir");
            std::fs::write(extract.dir().join("already.parquet"), b"rows").expect("rows");
        }
        (tmp, root)
    }

    #[tokio::test]
    async fn every_extract_already_filled_in_is_skipped_and_reported() {
        let rows = [
            recorded_row("20260927T090000Z", 9),
            recorded_row("20260927T190000Z", 19),
        ];
        let (_tmp, root) = store_filled_with(&rows).await;

        let backfilled = backfill_recorded(&root, &None, Utc::now())
            .await
            .expect("a backfill over the recorded extracts");

        assert_eq!(
            backfilled,
            Backfilled {
                filled: vec![],
                skipped: vec![
                    ExtractId::new("20260927T090000Z").unwrap(),
                    ExtractId::new("20260927T190000Z").unwrap(),
                ],
            }
        );
    }

    #[test]
    fn a_run_that_filled_or_skipped_nothing_says_so() {
        assert_eq!(named(&[]), "none");
        assert_eq!(
            named(&[ExtractId::new("20260927T090000Z").unwrap()]),
            "20260927T090000Z"
        );
    }

    #[test]
    fn taking_every_recorded_extract_again_is_the_default() {
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
