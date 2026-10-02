use std::io::Read;
use std::process::Command;

use clap::Parser;
use prose_gate::{StopPayload, block_reason, missing, skills_invoked};

/// A Claude Code Stop hook: a session that changed prose has to have invoked the writing skills.
#[derive(Parser)]
#[command(about, long_about = None)]
struct Args {
    /// A skill the session has to have invoked. Repeat for each.
    #[arg(long = "require", value_name = "SKILL", required = true)]
    required: Vec<String>,

    /// Which changed files count as prose, as a git pathspec.
    #[arg(long, value_name = "PATHSPEC", default_value = "*.md")]
    glob: String,
}

fn main() {
    let args = Args::parse();

    let mut stdin = String::new();
    if std::io::stdin().read_to_string(&mut stdin).is_err() {
        return;
    }
    let Ok(payload) = serde_json::from_str::<StopPayload>(&stdin) else {
        return;
    };
    if payload.block_already_fired() {
        return;
    }

    let cwd = payload.cwd.unwrap_or_else(|| ".".into());
    let Some(root) = git(&cwd, &["rev-parse", "--show-toplevel"]) else {
        return;
    };
    let root = root.trim();

    let Some(status) = git(root, &["status", "--porcelain", "--", &args.glob]) else {
        return;
    };
    let changed: Vec<String> = status.lines().map(str::to_string).collect();
    if changed.is_empty() {
        return;
    }

    let Some(transcript) = payload
        .transcript_path
        .and_then(|p| std::fs::read_to_string(p).ok())
    else {
        return;
    };

    let missing = missing(&args.required, &skills_invoked(&transcript));
    if missing.is_empty() {
        return;
    }

    let decision = serde_json::json!({
        "decision": "block",
        "reason": block_reason(&missing, &changed),
    });
    println!("{decision}");
}

fn git(dir: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
