use std::path::{Path, PathBuf};
use std::process::Command;

pub const ENV: &str = "BUILD_CODE_VERSION";

const CODE: [&str; 3] = ["crates", "Cargo.toml", "Cargo.lock"];
pub const DIRTY: &str = "-dirty";
pub const UNKNOWN: &str = "unknown";

pub fn emit() {
    let workspace = workspace();
    println!("cargo:rustc-env={ENV}={}", version(workspace.as_deref()));
    if let Some(workspace) = workspace {
        for path in CODE {
            println!("cargo:rerun-if-changed={}", workspace.join(path).display());
        }
        let branch = git(&workspace, &["symbolic-ref", "-q", "HEAD"]);
        for reference in ["index", "HEAD"].into_iter().chain(branch.as_deref()) {
            if let Some(path) = git(
                &workspace,
                &[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    reference,
                ],
            ) {
                println!("cargo:rerun-if-changed={path}");
            }
        }
    }
}

fn version(workspace: Option<&Path>) -> String {
    let Some(workspace) = workspace else {
        return UNKNOWN.to_string();
    };
    let hashes: Option<Vec<String>> = CODE
        .iter()
        .map(|path| {
            git(
                workspace,
                &["rev-parse", "--short=8", &format!("HEAD:./{path}")],
            )
        })
        .collect();
    let Some(hashes) = hashes else {
        return UNKNOWN.to_string();
    };
    let changed = git(
        workspace,
        &[&["status", "--porcelain", "--"][..], &CODE[..]].concat(),
    );
    match changed {
        Some(changes) if changes.is_empty() => hashes.join("."),
        _ => format!("{}{DIRTY}", hashes.join(".")),
    }
}

fn workspace() -> Option<PathBuf> {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
    manifest
        .ancestors()
        .find(|dir| dir.join("Cargo.lock").is_file())
        .map(Path::to_path_buf)
}

fn git(workspace: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .current_dir(workspace)
        .args(arguments)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}
