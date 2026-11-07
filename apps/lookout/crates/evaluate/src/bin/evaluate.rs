use std::error::Error;
use std::num::NonZeroUsize;

use clap::{Args, Parser, Subcommand};
use domain::{NotASpan, Percentage, Radius, Span};
use evaluation::gold;
use evaluation::record::CodeVersion;
use evaluation::steps::{Predictor, Run};
use evaluation::{Bootstrap, Pick, Score, Scoring, Seed};
use medallion::{Country, MedallionArgs, RunId};
use medallion_model::{EvaluationSummaryRowV1, FloatType, PredictorName};

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

#[derive(Args)]
struct Settings {
    /// The country whose train sessions are picked, such as `DE`.
    #[arg(long)]
    country: Country,
    /// The least share of a session's samples that come from realtime data, in percent.
    #[arg(long, value_parser = percentage)]
    realtime_at_least: Percentage,
    /// How many sessions to pick.
    #[arg(long)]
    sessions: NonZeroUsize,
    /// The seed the sessions are picked under.
    #[arg(long)]
    seed: u64,
    /// The predictor the sessions are replayed through: `oracle` or `crow_flies`.
    #[arg(long)]
    predictor: PredictorName,
    /// How far ahead of each pass the oracle alerts, such as `5m`.
    #[arg(long, value_parser = span, required_if_eq("predictor", "oracle"))]
    oracle_horizon: Option<Span>,
    /// How far from the latest sample CrowFlies predicts crossings, in metres.
    #[arg(long, required_if_eq("predictor", "crow_flies"))]
    radius_metres: Option<Radius>,
    /// The float type the replay runs in: `f32` or `f64`.
    #[arg(long)]
    float_type: FloatType,
    /// How far ahead of its pass a notification is a hit, such as `5m`.
    #[arg(long, value_parser = span)]
    horizon: Span,
    /// How close to its pass a notification is last moment, such as `30s`.
    #[arg(long, value_parser = span)]
    last_moment: Span,
    /// How far past the horizon a notification's score falls to the floor, such as `1m`.
    #[arg(long, value_parser = span)]
    ramp: Span,
    /// The score of a last-moment notification, from 0 to 1.
    #[arg(long, value_parser = score)]
    last_moment_score: Score,
    /// The score a notification too early falls to, from 0 to 1.
    #[arg(long, value_parser = score)]
    floor: Score,
    /// How long from a session's start its passes go unscored, such as `1m`.
    #[arg(long, value_parser = span)]
    warm_up: Span,
    /// How many resamples of the sessions the intervals are drawn from.
    #[arg(long)]
    resamples: NonZeroUsize,
    /// The seed the resamples are drawn under.
    #[arg(long)]
    bootstrap_seed: u64,
}

impl Settings {
    fn run(&self) -> Result<Run, &'static str> {
        let predictor = match (self.predictor, self.oracle_horizon, self.radius_metres) {
            (PredictorName::Oracle, Some(horizon), _) => Predictor::Oracle { horizon },
            (PredictorName::CrowFlies, _, Some(radius)) => Predictor::CrowFlies { radius },
            (PredictorName::Oracle, None, _) => return Err("the oracle needs --oracle-horizon"),
            (PredictorName::CrowFlies, _, None) => return Err("CrowFlies needs --radius-metres"),
        };
        Ok(Run {
            pick: Pick {
                country: self.country,
                realtime_at_least: self.realtime_at_least,
                sessions: self.sessions,
                seed: Seed(self.seed),
            },
            predictor,
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
        })
    }
}

fn span(text: &str) -> Result<Span, NotASpan> {
    Span::new(text)
}

fn percentage(text: &str) -> Result<Percentage, String> {
    let value: f64 = text.parse().map_err(|err| format!("{text:?}: {err}"))?;
    Percentage::new(value).map_err(|err| err.to_string())
}

fn score(text: &str) -> Result<Score, String> {
    let value: f64 = text.parse().map_err(|err| format!("{text:?}: {err}"))?;
    Score::new(value).map_err(|err| err.to_string())
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
            let (run_id, ran) = gold::run(&root, &settings.run()?, &code).await?;
            println!("run {run_id}");
            print_summary(&ran.summary);
        }
        Command::Pick(settings) => {
            let run_id = gold::pick(&root, &settings.run()?, &code).await?;
            println!("run {run_id}");
        }
        Command::Replay(begun) => gold::replay(&root, &begun.run_id()?, &code).await?,
        Command::Score(begun) => gold::score(&root, &begun.run_id()?, &code).await?,
        Command::Summarise(begun) => {
            print_summary(&gold::summarise(&root, &begun.run_id()?, &code).await?);
        }
    }
    Ok(())
}

impl Begun {
    fn run_id(&self) -> Result<RunId, medallion::PathError> {
        RunId::new(self.run_id.to_string())
    }
}

fn print_summary(summary: &EvaluationSummaryRowV1) {
    let estimate =
        |value: Option<f64>, low: Option<f64>, high: Option<f64>| match (value, low, high) {
            (Some(value), Some(low), Some(high)) => format!("{value:.3} [{low:.3}, {high:.3}]"),
            _ => "none".to_string(),
        };
    let lead = |milliseconds: Option<i64>| {
        milliseconds.map_or("none".to_string(), |milliseconds| {
            format!("{:.0} s", milliseconds as f64 / 1_000.0)
        })
    };
    println!(
        "sessions {}, {:.1} hours, {} passes, {} notifications",
        summary.sessions, summary.hours, summary.passes, summary.notifications
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
        "lead time p10 {}, median {}, p90 {}",
        lead(summary.lead_p10_ms),
        lead(summary.lead_median_ms),
        lead(summary.lead_p90_ms)
    );
}
