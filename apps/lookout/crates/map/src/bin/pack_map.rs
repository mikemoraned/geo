use std::error::Error;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use chrono::Utc;
use clap::Parser;
use map::base::write_base_map;
use map::journeys::write_journeys;
use map::tiles::{encode, tile};
use medallion::MedallionArgs;
use session_crossings::gold::{Choosing, choose};

const ARTIFACT: &str = "map";
const FILE: &str = "map.pmtiles";
const JOURNEYS: &str = "journeys.geojsonseq";
const BASE_MAP: &str = "base.geojsonseq";
const MVT: &str = "map.mvt.pmtiles";
const MAX_ZOOM: u8 = 12;

#[derive(Parser)]
#[command(
    about = "Pack the sessions worth replaying, with the rail and water the crossings name, into \
             PMTiles"
)]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
    /// How many crossings a session must pass to be worth replaying.
    #[arg(long, default_value_t = 5)]
    min_crossings: usize,
    /// How many of the best to keep, per country.
    #[arg(long, default_value_t = 3)]
    max_sessions: usize,
    /// Where to write it. Defaults to this run's artefact directory in the store's gold layer.
    #[arg(long)]
    output: Option<PathBuf>,
    /// A file to write this run's version into, naming what was just written. What reads it
    /// decides what to serve, so packing and adopting are one step.
    #[arg(long, conflicts_with = "output")]
    version_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pack_map=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args.medallion.root()?;
    let run = Utc::now();
    let output = match args.output {
        Some(path) => path,
        None => root.gold_artefact_dir(ARTIFACT, run)?,
    };
    let choosing = Choosing {
        min_crossings: args.min_crossings,
        max_sessions: args.max_sessions,
    };

    tracing::info!(
        medallion_root = %root.path().display(),
        output = %output.display(),
        min_crossings = choosing.min_crossings,
        max_sessions = choosing.max_sessions,
        "choosing sessions",
    );

    let replays = choose(&root, choosing).await?;
    let features = tempfile::tempdir()?;
    let journeys = features.path().join(JOURNEYS);
    let mut writing = BufWriter::new(File::create(&journeys)?);
    let written = write_journeys(&replays, &mut writing)?;
    writing.flush()?;

    tracing::info!(
        sessions = replays.len(),
        samples = replays
            .iter()
            .map(|replay| replay.samples.len())
            .sum::<usize>(),
        features = written,
        "wrote journeys",
    );

    let base_map = features.path().join(BASE_MAP);
    let mut writing = BufWriter::new(File::create(&base_map)?);
    let written = write_base_map(&root, &mut writing).await?;
    writing.flush()?;

    tracing::info!(
        rail = written.rail,
        water = written.water,
        bytes = base_map.metadata()?.len(),
        "wrote the base map",
    );

    let mvt = features.path().join(MVT);
    tile(&[&journeys, &base_map], MAX_ZOOM, &mvt)?;
    fs::create_dir_all(&output)?;
    let packed = output.join(FILE);
    encode(&mvt, &packed)?;

    let version = medallion::gold_version(run);
    if let Some(file) = &args.version_file {
        fs::write(file, format!("{version}\n"))?;
    }

    tracing::info!(
        path = %packed.display(),
        max_zoom = MAX_ZOOM,
        mvt_bytes = mvt.metadata()?.len(),
        bytes = packed.metadata()?.len(),
        %version,
        adopted = args.version_file.as_ref().map(|file| file.display().to_string()),
        "packed the map",
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn the_arguments_are_well_formed() {
        Args::command().debug_assert();
    }

    #[test]
    fn the_defaults_keep_the_best_three_of_those_passing_five() {
        let args = Args::parse_from(["pack_map"]);

        assert_eq!(args.min_crossings, 5);
        assert_eq!(args.max_sessions, 3);
        assert_eq!(args.output, None);
        assert_eq!(args.version_file, None);
    }

    #[test]
    fn a_redirected_run_cannot_also_adopt_a_version() {
        assert!(
            Args::try_parse_from([
                "pack_map",
                "--output",
                "/tmp/elsewhere",
                "--version-file",
                "map.version",
            ])
            .is_err()
        );
    }

    #[test]
    fn the_default_output_is_a_versioned_gold_artefact_of_whichever_store_is_read() {
        let args = Args::parse_from(["pack_map", "--medallion-root", "/somewhere/store"]);
        let root = args.medallion.root().unwrap();
        let run = Utc.with_ymd_and_hms(2026, 11, 15, 19, 48, 57).unwrap();

        assert_eq!(
            root.gold_artefact(ARTIFACT, run, FILE).unwrap(),
            PathBuf::from(
                "/somewhere/store/gold/artifact=map/version=20261115T194857000Z/map.pmtiles"
            )
        );
    }
}
