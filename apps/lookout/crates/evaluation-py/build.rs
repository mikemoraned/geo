use std::process::Command;

fn git(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").args(arguments).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn main() {
    let hash = match git(&["rev-parse", "--short", "HEAD"]) {
        Some(hash) => {
            let clean = Command::new("git")
                .args(["diff", "--quiet"])
                .status()
                .is_ok_and(|status| status.success());
            if clean { hash } else { format!("{hash}-dirty") }
        }
        None => "unknown".to_string(),
    };
    println!("cargo:rustc-env=BUILD_GIT_HASH={hash}");
    println!("cargo:rerun-if-changed=..");
    for reference in ["HEAD", "refs/heads"] {
        if let Some(path) = git(&["rev-parse", "--git-path", reference]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"])
        && let Some(path) = git(&["rev-parse", "--git-path", &branch])
    {
        println!("cargo:rerun-if-changed={path}");
    }
}
