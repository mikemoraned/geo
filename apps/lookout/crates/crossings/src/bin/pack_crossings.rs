//! `pack_crossings`: read the silver water crossings out of the store and write the flat
//! point buffer the M5 device scans.
//!
//! Every country the store holds is packed unless a window is given, since the device does
//! not know where it will be switched on.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use clap::Parser;
use crossings::{Bbox, Point, pointset, silver};
use medallion::MedallionArgs;

/// What the packed buffer is called in gold, and the file each version of it holds.
const ARTIFACT: &str = "crossings";
const FILE: &str = "crossings.pointset";

#[derive(Parser)]
#[command(about = "Pack silver water crossings into the M5 device's point buffer")]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
    /// Where to write the packed buffer. Defaults to the store's own gold layer.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Keep only crossings inside this `west,south,east,north` window. Omit to keep them all.
    #[arg(long)]
    bbox: Option<Bbox>,
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
    let output = match args.output {
        Some(path) => path,
        None => root.gold_artefact(ARTIFACT, Utc::now(), FILE)?,
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
            .filter(|crossing| window.contains(crossing.position.x, crossing.position.y))
            .collect(),
        None => read,
    };

    let points: Vec<_> = crossings.iter().map(Point::of).collect();

    let packed = pointset::pack(&points)?;
    if let Some(directory) = output.parent() {
        fs::create_dir_all(directory)?;
    }
    fs::write(&output, &packed)?;

    tracing::info!(
        crossings = crossings.len(),
        // Which extraction of the reference data the packed crossings came from, so a buffer
        // on a device can be traced back to a release. The format itself has no room for it.
        extracts = ?crossings
            .iter()
            .map(|crossing| crossing.extract_id.as_str())
            .collect::<BTreeSet<_>>(),
        bytes = packed.len(),
        "packed crossings",
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
    fn the_defaults_need_no_arguments() {
        let args = Args::parse_from(["pack_crossings"]);

        assert_eq!(args.output, None);
        assert_eq!(args.bbox, None);
    }

    /// The buffer belongs in the store it was derived from, under the run that produced it,
    /// so pointing a run at another store moves the output with it and a rerun leaves the
    /// last one where a device that holds it can still be traced to it.
    #[test]
    fn the_default_output_is_a_versioned_gold_artefact_of_whichever_store_is_read() {
        let args = Args::parse_from(["pack_crossings", "--medallion-root", "/somewhere/store"]);
        let root = args.medallion.root().unwrap();
        let run = Utc.with_ymd_and_hms(2026, 8, 1, 19, 48, 57).unwrap();

        assert_eq!(
            root.gold_artefact(ARTIFACT, run, FILE).unwrap(),
            PathBuf::from(
                "/somewhere/store/gold/artifact=crossings/version=20260801T194857000Z/crossings.pointset"
            )
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
