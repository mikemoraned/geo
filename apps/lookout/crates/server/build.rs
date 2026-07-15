fn main() {
    // BUILD_GIT_HASH is passed in by the build system (Justfile / Docker
    // `--build-arg`) rather than read from `.git`, which isn't in the Docker build
    // context. Fall back to "unknown" so a bare `cargo build` still compiles.
    let hash = std::env::var("BUILD_GIT_HASH")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_GIT_HASH={hash}");
    println!("cargo:rerun-if-env-changed=BUILD_GIT_HASH");
}
