use std::error::Error;
use std::num::NonZeroUsize;

use clap::{Parser, ValueEnum};
use domain::{Percentage, Span};
use evaluation::cone::{ConeAngles, ConeSettings, cone_angles};
use evaluation::store::read_samples;
use evaluation::{
    A5Resolution, Pick, Salt, ScoredSplit, Seed, Sessions, Split, Splitter, TuneBuckets, pick,
    resolve,
};
use medallion::{Country, MedallionArgs};
use statrs::statistics::{Data, Max, OrderStatistics};

#[derive(Parser)]
#[command(
    about = "Measure how far a train's later positions turn from its heading within a horizon, \
             over the windows of picked sessions that lie wholly in one split"
)]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
    /// The country whose train sessions are picked, such as `DE`.
    #[arg(long)]
    country: Country,
    /// The least share of a session's samples that come from realtime data, in percent.
    #[arg(long)]
    realtime_at_least: Percentage,
    /// How many sessions to pick, or `all` for every session the filter keeps.
    #[arg(long)]
    sessions: Sessions,
    /// The seed the sessions are picked under.
    #[arg(long)]
    seed: u64,
    /// The split whose windows are measured: `tune` or `test`.
    #[arg(long)]
    split: Measured,
    /// The A5 resolution of the cells the splitter assigns a split, such as `8`.
    #[arg(long)]
    split_resolution: A5Resolution,
    /// The salt the splitter hashes each cell with, such as `tune-test-1`.
    #[arg(long)]
    split_salt: Salt,
    /// How many of a hundred buckets the splitter puts in the tune split, such as `70`.
    #[arg(long)]
    tune_buckets: TuneBuckets,
    /// How far ahead of a window's start its later positions reach, such as `5m`.
    #[arg(long)]
    horizon: Span,
    /// How far a window's train travels, in metres, below which the window dwells and is left out.
    #[arg(long)]
    dwelling_metres: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Measured {
    Tune,
    Test,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let root = args.medallion.root()?;
    let sessions = resolve(&root, args.country, args.realtime_at_least, args.sessions).await?;
    let picked = pick(
        &root,
        &Pick {
            country: args.country,
            realtime_at_least: args.realtime_at_least,
            sessions,
            seed: Seed(args.seed),
        },
    )
    .await?;
    let samples = read_samples(
        &root,
        args.country,
        picked.iter().map(|session| &session.session_id),
    )
    .await?;
    let split = ScoredSplit {
        splitter: Splitter {
            resolution: args.split_resolution,
            salt: args.split_salt.clone(),
            tune_buckets: args.tune_buckets,
        },
        split: match args.split {
            Measured::Tune => Split::Tune,
            Measured::Test => Split::Test,
        },
    };
    let settings = ConeSettings {
        horizon: args.horizon.time_delta(),
        dwelling_metres: args.dwelling_metres,
    };
    let angles = samples
        .values()
        .map(|session| cone_angles(session, &split, &settings))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .fold(ConeAngles::default(), std::ops::Add::add);
    report(&angles, &args, sessions);
    Ok(())
}

fn report(angles: &ConeAngles, args: &Args, sessions: NonZeroUsize) {
    let window_angles = &angles.window_angles;
    println!("steps: {} samples of {sessions} sessions", angles.samples);
    println!(
        "  {} at exactly the coordinates of the sample before",
        angles.repeats_position
    );
    println!(
        "pairs: {} starts with a heading, {} pairs of a start and a later sample within {}",
        angles.starts, angles.pairs, args.horizon
    );
    println!(
        "windows: {}; {} lie wholly in {:?} cells, {} of them travelling under {} m and dwelling, \
         leaving {}",
        angles.windows,
        angles.in_split,
        args.split,
        angles.dwelling,
        args.dwelling_metres,
        angles.in_split - angles.dwelling
    );
    println!(
        "pair angles: {} pairs measured; {} left out, whose later sample lies at the start's \
         coordinates",
        angles.pair_angles, angles.pairs_without_bearing
    );
    let mut degrees = Data::new(
        window_angles
            .iter()
            .map(|window| window.degrees)
            .collect::<Vec<_>>(),
    );
    println!("window angles: {} windows", window_angles.len());
    for (name, quantile) in [
        ("p50", 0.5),
        ("p75", 0.75),
        ("p90", 0.9),
        ("p95", 0.95),
        ("p99", 0.99),
        ("p99.9", 0.999),
    ] {
        println!("  {name:>6} {:>6.1}°", degrees.quantile(quantile));
    }
    println!("  {:>6} {:>6.1}°", "largest", degrees.max());
}
