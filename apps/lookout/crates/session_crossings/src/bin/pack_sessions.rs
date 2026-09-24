use std::error::Error;
use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use clap::Parser;
use medallion::MedallionArgs;
use session_crossings::gold::{Choosing, choose};

const ARTIFACT: &str = "sessions";
const FILE: &str = "sessions.json";

#[derive(Parser)]
#[command(about = "Choose the recorded sessions worth replaying, and write them as JSON")]
struct Args {
    #[command(flatten)]
    medallion: MedallionArgs,
    /// How many crossings a session must pass to be worth replaying.
    #[arg(long, default_value_t = 5)]
    min_crossings: usize,
    /// How many of the best to keep.
    #[arg(long, default_value_t = 3)]
    max_sessions: usize,
    /// Where to write them. Defaults to this run's artefact directory in the store's gold
    /// layer.
    #[arg(long)]
    output: Option<PathBuf>,
    /// A file to write this run's version into, naming what was just written. What reads it
    /// decides what to serve, so choosing and adopting are one step.
    #[arg(long, conflicts_with = "output")]
    version_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pack_sessions=info".into()),
        )
        .init();

    let args = Args::parse();
    let root = args.medallion.root()?;
    let run = Utc::now();
    let output = match args.output {
        Some(path) => path,
        None => root
            .gold_artefact(ARTIFACT, run, FILE)?
            .parent()
            .expect("an artefact sits in a directory")
            .to_path_buf(),
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
    let json = serde_json::to_vec(&replays)?;

    fs::create_dir_all(&output)?;
    fs::write(output.join(FILE), &json)?;

    let version = medallion::gold_version(run);
    if let Some(file) = &args.version_file {
        fs::write(file, format!("{version}\n"))?;
    }

    tracing::info!(
        sessions = replays.len(),
        crossings = ?replays.iter().map(|replay| replay.crossings).collect::<Vec<_>>(),
        samples = replays.iter().map(|replay| replay.samples.len()).sum::<usize>(),
        bytes = json.len(),
        %version,
        adopted = args.version_file.as_ref().map(|file| file.display().to_string()),
        "chose sessions",
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn the_arguments_are_well_formed() {
        Args::command().debug_assert();
    }

    #[test]
    fn the_defaults_keep_the_best_three_of_those_passing_five() {
        let args = Args::parse_from(["pack_sessions"]);

        assert_eq!(args.min_crossings, 5);
        assert_eq!(args.max_sessions, 3);
        assert_eq!(args.output, None);
    }

    #[test]
    fn a_redirected_run_cannot_also_adopt_a_version() {
        assert!(
            Args::try_parse_from([
                "pack_sessions",
                "--output",
                "/tmp/elsewhere",
                "--version-file",
                "sessions.version",
            ])
            .is_err()
        );
    }
}
