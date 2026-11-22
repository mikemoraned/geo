use chrono::TimeDelta;
use std::error::Error;
use std::num::NonZeroUsize;

use clap::{Args, Parser, Subcommand, ValueEnum};
use code_version::CodeVersion;
use domain::{HalfWidth, Percentage, Radius, Span};
use evaluation::gold;
use evaluation::steps::{EvaluationSettings, Predictor};
use evaluation::{
    A5Resolution, Bootstrap, CrowFliesSettings, LightConeSettings, OnlineDetectorSettings,
    OracleSettings, Pick, Salt, Score, ScoredSplit, Scoring, Seed, Sessions, Split, Splitter,
    TuneBuckets, resolve,
};
use medallion::{Country, MedallionArgs, Root, RunId};
use medallion_model::{
    EvaluationSkewRowV2, EvaluationSplitRowV1, EvaluationSummaryRowV3, FloatType, PredictorName,
};

const BUILD_CODE_VERSION: &str = env!("BUILD_CODE_VERSION");

#[derive(Parser)]
#[command(about = "Evaluate a predictor over picked train sessions, writing each step to gold")]
struct Cli {
    #[command(flatten)]
    medallion: MedallionArgs,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Pick sessions, then replay, score, and summarise them, as one run.
    Run(Settings),
    /// Pick sessions, and record the run's settings and inputs.
    Pick(Settings),
    /// Replay a picked run's sessions through its predictor.
    Replay(Begun),
    /// Score a replayed run's notifications against its sessions' passes.
    Score(Begun),
    /// Summarise a scored run into its measures and their intervals.
    Summarise(Begun),
}

#[derive(Args)]
struct Begun {
    /// The run, as `pick` named it.
    #[arg(long)]
    run_id: uuid::Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Scored {
    Tune,
    Test,
    Every,
}

#[derive(Args)]
struct Settings {
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
    /// The predictor the sessions are replayed through: `oracle`, `crow_flies`, or `light_cone`.
    #[arg(long)]
    predictor: PredictorName,
    /// How far ahead the predictor alerts a pass, such as `5m`.
    #[arg(long)]
    predictor_horizon: Span,
    /// How far from the latest sample CrowFlies or LightCone predicts crossings, in metres.
    #[arg(long, required_if_eq_any([("predictor", "crow_flies"), ("predictor", "light_cone")]))]
    radius_metres: Option<Radius>,
    /// How far either side of the train's heading LightCone's cone reaches, in degrees.
    #[arg(long, required_if_eq("predictor", "light_cone"))]
    half_width_degrees: Option<HalfWidth>,
    /// How far, in metres, a train moves from where it last moved before CrowFlies or LightCone
    /// takes its speed and heading from the move.
    #[arg(long, required_if_eq_any([("predictor", "crow_flies"), ("predictor", "light_cone")]))]
    still_within_metres: Option<Radius>,
    /// How near a sample the online pass detector reports a crossing as passing, in metres. The
    /// run measures how far the passes it reports skew from ground truth.
    #[arg(long)]
    online_pass_radius_metres: Radius,
    /// The float type the replay runs in: `f32` or `f64`.
    #[arg(long)]
    float_type: FloatType,
    /// How far ahead of its pass a notification is a hit, such as `5m`.
    #[arg(long)]
    horizon: Span,
    /// How close to its pass a notification is last moment, such as `30s`.
    #[arg(long)]
    last_moment: Span,
    /// How far past the horizon a notification's score falls to the floor, such as `1m`.
    #[arg(long)]
    ramp: Span,
    /// The score of a last-moment notification, from 0 to 1.
    #[arg(long)]
    last_moment_score: Score,
    /// The score a notification too early falls to, from 0 to 1.
    #[arg(long)]
    floor: Score,
    /// How long from a session's start its passes go unscored, such as `1m`.
    #[arg(long)]
    warm_up: Span,
    /// How many resamples of the sessions the intervals are drawn from.
    #[arg(long)]
    resamples: NonZeroUsize,
    /// The seed the resamples are drawn under.
    #[arg(long)]
    bootstrap_seed: u64,
    /// The crossings the run scores: those of the `tune` split, of the `test` split, or `every`
    /// crossing.
    #[arg(long)]
    split: Scored,
    /// The A5 resolution of the cells the splitter assigns a split, such as `8`.
    #[arg(long, required_if_eq_any([("split", "tune"), ("split", "test")]))]
    split_resolution: Option<A5Resolution>,
    /// The salt the splitter hashes each cell with, such as `tune-test-1`.
    #[arg(long, required_if_eq_any([("split", "tune"), ("split", "test")]))]
    split_salt: Option<Salt>,
    /// How many of a hundred buckets the splitter puts in the tune split, such as `70`.
    #[arg(long, required_if_eq_any([("split", "tune"), ("split", "test")]))]
    tune_buckets: Option<TuneBuckets>,
}

impl Settings {
    async fn resolved(&self, root: &Root) -> Result<EvaluationSettings, Box<dyn Error>> {
        let sessions = resolve(root, self.country, self.realtime_at_least, self.sessions).await?;
        Ok(self.run(sessions)?)
    }

    fn run(&self, sessions: NonZeroUsize) -> Result<EvaluationSettings, &'static str> {
        let horizon = self.predictor_horizon;
        let predictor = match self.predictor {
            PredictorName::Oracle => Predictor::Oracle(OracleSettings { horizon }),
            PredictorName::CrowFlies => Predictor::CrowFlies(CrowFliesSettings {
                radius: self
                    .radius_metres
                    .ok_or("CrowFlies needs --radius-metres")?,
                horizon,
                still_within: self
                    .still_within_metres
                    .ok_or("CrowFlies needs --still-within-metres")?,
            }),
            PredictorName::LightCone => Predictor::LightCone(LightConeSettings {
                radius: self
                    .radius_metres
                    .ok_or("LightCone needs --radius-metres")?,
                half_width: self
                    .half_width_degrees
                    .ok_or("LightCone needs --half-width-degrees")?,
                horizon,
                still_within: self
                    .still_within_metres
                    .ok_or("LightCone needs --still-within-metres")?,
            }),
        };
        Ok(EvaluationSettings {
            pick: Pick {
                country: self.country,
                realtime_at_least: self.realtime_at_least,
                sessions,
                seed: Seed(self.seed),
            },
            predictor,
            online_detector: Some(OnlineDetectorSettings {
                pass_radius: self.online_pass_radius_metres,
            }),
            float_type: self.float_type,
            scoring: Scoring {
                horizon: self.horizon,
                last_moment: self.last_moment,
                ramp: self.ramp,
                last_moment_score: self.last_moment_score,
                floor: self.floor,
                warm_up: self.warm_up,
            },
            bootstrap: Bootstrap {
                resamples: self.resamples,
                seed: Seed(self.bootstrap_seed),
            },
            split: scored_split(
                self.split,
                self.split_resolution,
                self.split_salt.clone(),
                self.tune_buckets,
            )?,
        })
    }
}

fn scored_split(
    scored: Scored,
    resolution: Option<A5Resolution>,
    salt: Option<Salt>,
    tune_buckets: Option<TuneBuckets>,
) -> Result<Option<ScoredSplit>, &'static str> {
    let split = match scored {
        Scored::Tune => Split::Tune,
        Scored::Test => Split::Test,
        Scored::Every => {
            return match (resolution, salt, tune_buckets) {
                (None, None, None) => Ok(None),
                _ => Err("a run of every crossing takes no splitter"),
            };
        }
    };
    match (resolution, salt, tune_buckets) {
        (Some(resolution), Some(salt), Some(tune_buckets)) => Ok(Some(ScoredSplit {
            splitter: Splitter {
                resolution,
                salt,
                tune_buckets,
            },
            split,
        })),
        _ => Err("a split needs --split-resolution, --split-salt, and --tune-buckets"),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "evaluation=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let root = cli.medallion.root()?;
    let code = CodeVersion::new(BUILD_CODE_VERSION)?;

    match cli.command {
        Command::Run(settings) => {
            let (run_id, results) =
                gold::run(&root, &settings.resolved(&root).await?, &code).await?;
            println!("run {run_id}");
            print_summary(&results.summary);
            print_skew(results.skew.as_ref());
            print_split(results.split.as_ref());
        }
        Command::Pick(settings) => {
            let run_id = gold::pick(&root, &settings.resolved(&root).await?, &code).await?;
            println!("run {run_id}");
        }
        Command::Replay(begun) => {
            print_skew(gold::replay(&root, &begun.run_id(), &code).await?.as_ref())
        }
        Command::Score(begun) => gold::score(&root, &begun.run_id(), &code).await?,
        Command::Summarise(begun) => {
            print_summary(&gold::summarise(&root, &begun.run_id(), &code).await?);
        }
    }
    Ok(())
}

impl Begun {
    fn run_id(&self) -> RunId {
        RunId::from(self.run_id)
    }
}

fn estimate(value: Option<f64>, low: Option<f64>, high: Option<f64>) -> String {
    match (value, low, high) {
        (Some(value), Some(low), Some(high)) => format!("{value:.3} [{low:.3}, {high:.3}]"),
        _ => "none".to_string(),
    }
}

fn seconds(duration: Option<TimeDelta>) -> String {
    duration.map_or("none".to_string(), |duration| {
        format!("{:.0} s", duration.as_seconds_f64())
    })
}

fn print_skew(skew: Option<&EvaluationSkewRowV2>) {
    let Some(skew) = skew else {
        println!("skew not measured: the run has no online pass detector");
        return;
    };
    println!("skew of the online pass detector from ground truth, apart from the predictor:");
    println!(
        "  {} passes, {} seen; {} visits, {} unmatched",
        skew.passes, skew.seen_passes, skew.visits, skew.unmatched_visits
    );
    println!(
        "  seen {}",
        estimate(skew.seen, skew.seen_low, skew.seen_high)
    );
    println!(
        "  unmatched {}",
        estimate(skew.unmatched, skew.unmatched_low, skew.unmatched_high)
    );
    println!(
        "  offset p10 {}, median {}, p90 {}",
        seconds(skew.offset_p10),
        seconds(skew.offset_median),
        seconds(skew.offset_p90)
    );
}

fn print_split(split: Option<&EvaluationSplitRowV1>) {
    let Some(split) = split else {
        println!("split not checked: the run scores every crossing");
        return;
    };
    println!("split checked against the store:");
    println!(
        "  {} of {} cells and {} of {} crossings in the split",
        split.cells_in_split, split.cells, split.crossings_in_split, split.crossings
    );
    println!(
        "  passes {} in, {} out; outcomes {} in, {} out",
        split.passes_in_split,
        split.passes_outside,
        split.outcomes_in_split,
        split.outcomes_outside
    );
}

fn print_summary(summary: &EvaluationSummaryRowV3) {
    println!(
        "sessions {}, {:.1} hours, {} passes, {} notifications",
        summary.sessions,
        summary.scored_duration.as_seconds_f64() / 3_600.0,
        summary.passes,
        summary.notifications
    );
    println!(
        "false alarms {}, repeats {}, misses {}",
        summary.false_alarms, summary.repeats, summary.misses
    );
    println!(
        "precision {}",
        estimate(
            summary.precision,
            summary.precision_low,
            summary.precision_high
        )
    );
    println!(
        "recall {}",
        estimate(summary.recall, summary.recall_low, summary.recall_high)
    );
    println!(
        "false alarms per hour {}",
        estimate(
            summary.false_alarms_per_hour,
            summary.false_alarms_per_hour_low,
            summary.false_alarms_per_hour_high
        )
    );
    println!(
        "F0.5 {}",
        estimate(summary.f_half, summary.f_half_low, summary.f_half_high)
    );
    println!(
        "lead time p10 {}, median {}, p90 {}",
        seconds(summary.lead_p10),
        seconds(summary.lead_median),
        seconds(summary.lead_p90)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn splitter() -> (Option<A5Resolution>, Option<Salt>, Option<TuneBuckets>) {
        (
            Some(A5Resolution::new(8).expect("a resolution")),
            Some(Salt::new("tune-test-1").expect("a salt")),
            Some(TuneBuckets::new(70).expect("tune buckets")),
        )
    }

    #[test]
    fn a_run_of_the_test_split_scores_that_split_of_the_splitters_cells() {
        let (resolution, salt, tune_buckets) = splitter();

        assert_eq!(
            scored_split(Scored::Test, resolution, salt, tune_buckets),
            Ok(Some(ScoredSplit {
                splitter: Splitter {
                    resolution: A5Resolution::new(8).expect("a resolution"),
                    salt: Salt::new("tune-test-1").expect("a salt"),
                    tune_buckets: TuneBuckets::new(70).expect("tune buckets"),
                },
                split: Split::Test,
            }))
        );
    }

    #[test]
    fn a_run_of_every_crossing_has_no_split() {
        assert_eq!(scored_split(Scored::Every, None, None, None), Ok(None));
    }

    #[test]
    fn a_run_of_every_crossing_given_a_splitter_is_refused() {
        let (resolution, salt, tune_buckets) = splitter();

        assert!(scored_split(Scored::Every, resolution, salt, tune_buckets).is_err());
    }

    #[test]
    fn the_flags_name_the_chosen_splitter() {
        let cli = Cli::try_parse_from([
            "evaluate",
            "run",
            "--country",
            "DE",
            "--realtime-at-least",
            "90",
            "--sessions",
            "1000",
            "--seed",
            "1",
            "--predictor",
            "oracle",
            "--predictor-horizon",
            "5m",
            "--online-pass-radius-metres",
            "100",
            "--float-type",
            "f32",
            "--horizon",
            "5m",
            "--last-moment",
            "30s",
            "--ramp",
            "1m",
            "--last-moment-score",
            "0.25",
            "--floor",
            "0.05",
            "--warm-up",
            "1m",
            "--resamples",
            "1000",
            "--bootstrap-seed",
            "1",
            "--split",
            "test",
            "--split-resolution",
            "8",
            "--split-salt",
            "tune-test-1",
            "--tune-buckets",
            "70",
        ])
        .expect("the flags parse");

        let Command::Run(settings) = cli.command else {
            panic!("a run");
        };
        assert_eq!(
            settings
                .run(NonZeroUsize::new(1000).expect("not zero"))
                .map(|settings| settings.split),
            Ok(Some(ScoredSplit {
                splitter: Splitter {
                    resolution: A5Resolution::new(8).expect("a resolution"),
                    salt: Salt::new("tune-test-1").expect("a salt"),
                    tune_buckets: TuneBuckets::new(70).expect("tune buckets"),
                },
                split: Split::Test,
            }))
        );
    }

    #[test]
    fn a_split_without_its_splitter_is_refused_by_the_flags() {
        let parsed = Cli::try_parse_from([
            "evaluate",
            "pick",
            "--country",
            "DE",
            "--realtime-at-least",
            "90",
            "--sessions",
            "1000",
            "--seed",
            "1",
            "--predictor",
            "oracle",
            "--predictor-horizon",
            "5m",
            "--online-pass-radius-metres",
            "100",
            "--float-type",
            "f32",
            "--horizon",
            "5m",
            "--last-moment",
            "30s",
            "--ramp",
            "1m",
            "--last-moment-score",
            "0.25",
            "--floor",
            "0.05",
            "--warm-up",
            "1m",
            "--resamples",
            "1000",
            "--bootstrap-seed",
            "1",
            "--split",
            "tune",
        ]);

        assert!(parsed.is_err());
    }

    fn light_cone_flags(predictor: &[&str]) -> Vec<String> {
        [
            "evaluate",
            "run",
            "--country",
            "DE",
            "--realtime-at-least",
            "90",
            "--sessions",
            "1000",
            "--seed",
            "1",
            "--predictor",
            "light_cone",
            "--predictor-horizon",
            "5m",
            "--online-pass-radius-metres",
            "100",
            "--float-type",
            "f32",
            "--horizon",
            "5m",
            "--last-moment",
            "30s",
            "--ramp",
            "1m",
            "--last-moment-score",
            "0.25",
            "--floor",
            "0.05",
            "--warm-up",
            "1m",
            "--resamples",
            "1000",
            "--bootstrap-seed",
            "1",
            "--split",
            "every",
        ]
        .into_iter()
        .chain(predictor.iter().copied())
        .map(str::to_string)
        .collect()
    }

    #[test]
    fn the_flags_name_light_cones_settings() {
        let cli = Cli::try_parse_from(light_cone_flags(&[
            "--radius-metres",
            "20000",
            "--half-width-degrees",
            "45",
            "--still-within-metres",
            "20",
        ]))
        .expect("the flags parse");

        let Command::Run(settings) = cli.command else {
            panic!("a run");
        };
        assert_eq!(
            settings
                .run(NonZeroUsize::new(1000).expect("not zero"))
                .map(|settings| settings.predictor),
            Ok(Predictor::LightCone(LightConeSettings {
                radius: Radius::new(20_000.0).expect("a radius"),
                half_width: HalfWidth::new(45.0).expect("a half-width"),
                horizon: Span::new("5m").expect("a span"),
                still_within: Radius::new(20.0).expect("a radius"),
            }))
        );
    }

    #[test]
    fn light_cone_without_a_half_width_is_refused_by_the_flags() {
        let parsed = Cli::try_parse_from(light_cone_flags(&[
            "--radius-metres",
            "20000",
            "--still-within-metres",
            "20",
        ]));

        assert!(parsed.is_err());
    }
}
