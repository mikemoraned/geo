use std::path::{Path, PathBuf};
use std::process::Command;

pub const ENV: &str = "BUILD_CODE_VERSION";

const CODE: [&str; 3] = ["crates", "Cargo.toml", "Cargo.lock"];
const DIRTY: &str = "-dirty";
const UNKNOWN: &str = "unknown";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeVersion(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodeVersionError {
    #[error(
        "the code was built with uncommitted changes ({hash}), so it has no version to name; \
         commit them and build again"
    )]
    Uncommitted { hash: String },
    #[error("the build recorded no code version")]
    Unknown,
}

impl CodeVersion {
    pub fn new(hash: &str) -> Result<Self, CodeVersionError> {
        if hash.is_empty() || hash == UNKNOWN {
            return Err(CodeVersionError::Unknown);
        }
        if hash.ends_with(DIRTY) {
            return Err(CodeVersionError::Uncommitted {
                hash: hash.to_string(),
            });
        }
        Ok(Self(hash.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_of_committed_code_is_named_as_it_reads() {
        let version = CodeVersion::new("1a2b3c4d.5e6f7a8b.9c0d1e2f").expect("a code version");

        assert_eq!(version.as_str(), "1a2b3c4d.5e6f7a8b.9c0d1e2f");
    }

    #[test]
    fn a_version_of_uncommitted_code_is_refused() {
        assert_eq!(
            CodeVersion::new("1a2b3c4d-dirty"),
            Err(CodeVersionError::Uncommitted {
                hash: "1a2b3c4d-dirty".to_string()
            })
        );
    }

    #[test]
    fn a_build_with_no_version_is_refused() {
        for hash in ["", "unknown"] {
            assert_eq!(
                CodeVersion::new(hash),
                Err(CodeVersionError::Unknown),
                "{hash:?}"
            );
        }
    }
}
