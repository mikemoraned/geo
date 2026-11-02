use clap::Parser;
use tracing_subscriber::EnvFilter;

use domain::Radius;
use medallion::MedallionArgs;
use session_crossings::matching::DEFAULT_RADIUS;
use session_crossings::silver;

#[derive(Parser)]
#[command(about = "Derive the crossings each session passed")]
struct Args {
    /// How near a sample has to come to a crossing, in metres, for it to count as passed.
    #[arg(long, default_value_t = DEFAULT_RADIUS)]
    match_radius_m: Radius,
    #[command(flatten)]
    medallion: MedallionArgs,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "match_crossings=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args.medallion.root().expect("locate the medallion store");

    let outcome = silver::derive(&root, args.match_radius_m)
        .await
        .expect("derive the crossings each session passed");

    tracing::info!(
        sessions = outcome.sessions,
        crossings = outcome.crossings,
        sessions_matched = outcome.sessions_matched,
        passes = outcome.passes,
        partitions = outcome.partitions.written,
        partitions_removed = outcome.partitions.removed,
        match_radius_m = args.match_radius_m.as_metres(),
        medallion_root = %root.path().display(),
        "derived the crossings each session passed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_radius_defaults_to_the_default_radius() {
        let args = Args::parse_from(["match_crossings"]);

        assert_eq!(args.match_radius_m, DEFAULT_RADIUS);
    }

    #[test]
    fn a_radius_is_read_in_metres() {
        let args = Args::parse_from(["match_crossings", "--match-radius-m", "100"]);

        assert_eq!(args.match_radius_m.as_metres(), 100.0);
    }

    #[test]
    fn a_radius_that_is_not_positive_and_finite_is_refused() {
        for metres in ["0", "-250", "inf", "NaN", "far"] {
            let flag = format!("--match-radius-m={metres}");
            assert!(
                Args::try_parse_from(["match_crossings", flag.as_str()]).is_err(),
                "{metres}"
            );
        }
    }
}
