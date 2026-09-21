//! `pack_crossings`: read the silver water crossings out of the store and write the two forms
//! a shell predicts against — the flat buffer the M5 device scans in flash, and the array a
//! browser fetches.
//!
//! Both come from one read, so the crossings a device carries and the crossings a page draws
//! cannot disagree about which places exist. They differ only in precision: the buffer holds
//! `f32`, which is what the board's FPU measures in, and the array holds the degrees silver
//! recorded.
//!
//! Every country the store holds is packed unless a window is given, since neither shell knows
//! where it will be switched on.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use clap::Parser;
use crossings::{pointset, silver};
use domain::Bbox;
use medallion::MedallionArgs;

/// What the crossings are called in gold, and the files each version of them holds.
const ARTIFACT: &str = "crossings";
const PACKED: &str = "crossings.pointset";
const ARRAY: &str = "crossings.json";

/// Decimal places kept in the array, worth about 11cm of latitude.
///
/// Silver holds a position as `f64` and writing one out takes seventeen significant digits,
/// which is nanometres and sixteen characters a coordinate. The buffer rounds the same
/// position to `f32`, worth about 40cm, and no fix is that good either.
const PLACES: f64 = 1e6;

#[derive(Parser)]
#[command(about = "Pack silver water crossings into the M5 device's point buffer")]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
    /// Where to write them. Defaults to this run's artefact directory in the store's gold
    /// layer.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Keep only crossings inside this `west,south,east,north` window. Omit to keep them all.
    #[arg(long)]
    bbox: Option<Bbox>,
    /// A file to write this run's version into, naming what was just packed. What reads it
    /// decides what to build against, so packing and adopting are one step.
    #[arg(long, conflicts_with = "output")]
    version_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pack_crossings=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args.medallion.root()?;
    let run = Utc::now();
    let output = match args.output {
        Some(path) => path,
        None => root
            .gold_artefact(ARTIFACT, run, PACKED)?
            .parent()
            .expect("an artefact sits in a directory")
            .to_path_buf(),
    };

    tracing::info!(
        medallion_root = %root.path().display(),
        output = %output.display(),
        bbox = args.bbox.map(|bbox| bbox.to_string()),
        "packing crossings",
    );

    let read = silver::read(&root).await?;
    let crossings: Vec<_> = match args.bbox {
        Some(window) => read
            .into_iter()
            .filter(|crossing| {
                window.contains(crossing.crossing.longitude(), crossing.crossing.latitude())
            })
            .collect(),
        None => read,
    };

    let points: Vec<_> = crossings
        .iter()
        .map(crossings::compacted)
        .collect::<Result<Vec<_>, _>>()?;
    let packed = pointset::pack(&points)?;

    // The degrees silver recorded, not the `f32` the buffer rounds them to: a browser has no
    // reason to inherit the board's precision, only to stop short of absurd.
    let array: Vec<domain::CrossingCompact<f64>> = crossings
        .iter()
        .map(|crossing| {
            domain::CrossingCompact::at(
                crossing.compact_id,
                round(crossing.crossing.latitude()),
                round(crossing.crossing.longitude()),
            )
        })
        .collect::<Result<_, _>>()?;
    let json = serde_json::to_vec(&array)?;

    fs::create_dir_all(&output)?;
    fs::write(output.join(PACKED), &packed)?;
    fs::write(output.join(ARRAY), &json)?;

    // Last, so a run that failed to write its artefacts does not leave something pointing at
    // a version that is not there.
    let version = medallion::gold_version(run);
    if let Some(file) = &args.version_file {
        fs::write(file, format!("{version}\n"))?;
    }

    tracing::info!(
        crossings = crossings.len(),
        // Which extraction of the reference data the packed crossings came from, so a buffer
        // on a device can be traced back to a release. The format itself has no room for it.
        extracts = ?crossings
            .iter()
            .map(|crossing| crossing.extract_id.as_str())
            .collect::<BTreeSet<_>>(),
        packed_bytes = packed.len(),
        json_bytes = json.len(),
        %version,
        adopted = args.version_file.as_ref().map(|file| file.display().to_string()),
        "packed crossings",
    );

    Ok(())
}

/// A coordinate at the precision the array keeps.
fn round(degrees: f64) -> f64 {
    (degrees * PLACES).round() / PLACES
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use clap::CommandFactory;
    use medallion::Root;

    use super::*;

    #[test]
    fn the_arguments_are_well_formed() {
        Args::command().debug_assert();
    }

    #[test]
    fn the_defaults_need_no_arguments() {
        let args = Args::parse_from(["pack_crossings"]);

        assert_eq!(args.output, None);
        assert_eq!(args.bbox, None);
    }

    /// The crossings belong in the store they were derived from, under the run that produced
    /// them, so pointing a run at another store moves the output with it and a rerun leaves
    /// the last one where a device that holds it can still be traced to it.
    #[test]
    fn the_default_output_is_a_versioned_gold_artefact_of_whichever_store_is_read() {
        let args = Args::parse_from(["pack_crossings", "--medallion-root", "/somewhere/store"]);
        let root = args.medallion.root().unwrap();
        let run = Utc.with_ymd_and_hms(2026, 8, 1, 19, 48, 57).unwrap();

        assert_eq!(
            root.gold_artefact(ARTIFACT, run, PACKED).unwrap(),
            PathBuf::from(
                "/somewhere/store/gold/artifact=crossings/version=20260801T194857000Z/crossings.pointset"
            )
        );
    }

    /// Eleven centimetres, and short enough to write.
    #[test]
    fn a_coordinate_is_kept_to_six_places() {
        assert_eq!(round(50.772_051_974_934_95), 50.772_052);
        assert_eq!(round(13.089_276_802_196_796), 13.089_277);
    }

    /// Adopting means naming the version that was written, so a run sending its artefacts
    /// somewhere else has no version to adopt.
    #[test]
    fn a_redirected_run_cannot_also_adopt_a_version() {
        assert!(
            Args::try_parse_from([
                "pack_crossings",
                "--output",
                "/tmp/elsewhere",
                "--version-file",
                "crossings.version",
            ])
            .is_err()
        );
    }

    #[test]
    fn the_version_adopted_is_the_one_the_path_was_built_from() {
        let run = Utc.with_ymd_and_hms(2026, 8, 1, 19, 48, 57).unwrap();

        let version = medallion::gold_version(run);

        assert_eq!(version, "20260801T194857000Z");
        assert!(
            Root::new("/somewhere/store")
                .gold_artefact(ARTIFACT, run, PACKED)
                .unwrap()
                .to_string_lossy()
                .contains(&format!("version={version}"))
        );
    }

    #[test]
    fn a_window_is_parsed_and_validated_by_clap() {
        let args = Args::parse_from(["pack_crossings", "--bbox", "6.08,47.42,15.04,54.93"]);

        assert_eq!(
            args.bbox,
            Some(Bbox::new(6.08, 47.42, 15.04, 54.93).unwrap())
        );
        assert!(Args::try_parse_from(["pack_crossings", "--bbox", "6.08,47.42"]).is_err());
    }
}
