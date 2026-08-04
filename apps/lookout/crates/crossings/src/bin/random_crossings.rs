//! `random_crossings`: write a point buffer of made-up crossings, for trying the device out
//! before the real ones are on it. Reproducible from the seed.
//!
//! The spike that scans them embeds the file at build time, so it has to be rebuilt and
//! reflashed for a new set to reach the device.

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use clap::Parser;
use crossings::{Bbox, pointset, random};

/// As many as the real German crossings set holds, since the question the made-up set answers
/// is what a scan of that many points costs.
const DEFAULT_COUNT: usize = 5_749;
/// The real set's extent, so distances between made-up points are of realistic size.
const DEFAULT_BBOX: &str = "6.08,47.42,15.04,54.93";
const DEFAULT_SEED: u64 = 5749;

#[derive(Parser)]
#[command(about = "Write a point buffer of made-up crossings")]
struct Args {
    /// Where to write the packed buffer.
    #[arg(long)]
    output: PathBuf,
    /// How many crossings to make up.
    #[arg(long, default_value_t = DEFAULT_COUNT)]
    count: usize,
    /// Scatter them through this `west,south,east,north` window.
    #[arg(long, default_value = DEFAULT_BBOX)]
    bbox: Bbox,
    /// The same seed always gives the same crossings.
    #[arg(long, default_value_t = DEFAULT_SEED)]
    seed: u64,
}

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "random_crossings=info".into()),
        )
        .init();

    let args = Args::parse();

    let points = random::points(args.count, &args.bbox, args.seed);
    let packed = pointset::pack(&points)?;

    if let Some(directory) = args.output.parent() {
        fs::create_dir_all(directory)?;
    }
    fs::write(&args.output, &packed)?;

    tracing::info!(
        output = %args.output.display(),
        crossings = points.len(),
        bbox = %args.bbox,
        seed = args.seed,
        bytes = packed.len(),
        "wrote made-up crossings",
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
    fn the_defaults_match_the_real_set() {
        let args = Args::parse_from(["random_crossings", "--output", "out.pointset"]);

        assert_eq!(args.count, DEFAULT_COUNT);
        assert_eq!(args.seed, DEFAULT_SEED);
        assert_eq!(args.bbox, DEFAULT_BBOX.parse().unwrap());
    }

    /// Without an output path there is nowhere to put the buffer, and defaulting one would
    /// invite overwriting a real set with a made-up one.
    #[test]
    fn an_output_path_is_required() {
        assert!(Args::try_parse_from(["random_crossings"]).is_err());
    }
}
