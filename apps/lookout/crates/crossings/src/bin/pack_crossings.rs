//! `pack_crossings`: read the silver water-crossings GeoParquet and write the flat point
//! buffer the M5 device scans.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use clap::Parser;
use crossings::{Bbox, Point, id, pointset, silver};

/// The water_crossings notebook's representative-point export: one row per crossing.
const DEFAULT_INPUT: &str = "data/water/v8/crossing_reps.parquet";
/// Gold, and regenerable from silver — gitignored like the notebook's own outputs.
const DEFAULT_OUTPUT: &str = "data/gold/crossings.pointset";

#[derive(Parser)]
#[command(about = "Pack silver water crossings into the M5 device's point buffer")]
struct Args {
    /// GeoParquet to read the crossings from.
    #[arg(long, default_value = DEFAULT_INPUT)]
    input: PathBuf,
    /// Where to write the packed buffer.
    #[arg(long, default_value = DEFAULT_OUTPUT)]
    output: PathBuf,
    /// Keep only crossings inside this `west,south,east,north` window. Omit to keep them all.
    #[arg(long)]
    bbox: Option<Bbox>,
}

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pack_crossings=info".into()),
        )
        .init();

    let args = Args::parse();

    tracing::info!(
        input = %args.input.display(),
        output = %args.output.display(),
        bbox = args.bbox.map(|bbox| bbox.to_string()),
        "packing crossings",
    );

    let read = silver::read(&args.input)?;
    let crossings: Vec<_> = match args.bbox {
        Some(window) => read
            .into_iter()
            .filter(|crossing| window.contains(crossing.position.x, crossing.position.y))
            .collect(),
        None => read,
    };

    let ids = id::assign(&crossings)?;
    let points: Vec<_> = crossings
        .iter()
        .zip(&ids)
        .map(|(crossing, id)| Point::of(crossing, *id))
        .collect();

    let packed = pointset::pack(&points)?;
    if let Some(directory) = args.output.parent() {
        fs::create_dir_all(directory)?;
    }
    fs::write(&args.output, &packed)?;

    tracing::info!(
        crossings = crossings.len(),
        distinct = ids.iter().collect::<BTreeSet<_>>().len(),
        bytes = packed.len(),
        "packed crossings",
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_arguments_are_well_formed() {
        Args::command().debug_assert();
    }

    #[test]
    fn the_defaults_need_no_arguments() {
        let args = Args::parse_from(["pack_crossings"]);

        assert_eq!(args.input, PathBuf::from(DEFAULT_INPUT));
        assert_eq!(args.output, PathBuf::from(DEFAULT_OUTPUT));
        assert_eq!(args.bbox, None);
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
