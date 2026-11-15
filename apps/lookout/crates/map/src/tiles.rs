use std::io;
use std::path::Path;
use std::process::{Command, ExitStatus};

#[derive(Debug, thiserror::Error)]
pub enum TileError {
    #[error("running {tool}: {source}")]
    Spawn {
        tool: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("{tool} exited with {status}: {stderr}")]
    Failed {
        tool: &'static str,
        status: ExitStatus,
        stderr: String,
    },
}

pub fn tile(features: &[&Path], max_zoom: u8, mvt: &Path) -> Result<(), TileError> {
    run(
        "tippecanoe",
        Command::new("tippecanoe")
            .arg("--output")
            .arg(mvt)
            .arg("--minimum-zoom=0")
            .arg(format!("--maximum-zoom={max_zoom}"))
            .arg("--drop-rate=1")
            .arg("--no-feature-limit")
            .arg("--no-tile-size-limit")
            .arg("--read-parallel")
            .arg("--quiet")
            .args(features),
    )
}

pub fn encode(mvt: &Path, mlt: &Path) -> Result<(), TileError> {
    run(
        "mlt",
        Command::new("mlt")
            .arg("convert")
            .arg("--profile=web")
            .arg(mvt)
            .arg(mlt),
    )
}

fn run(tool: &'static str, command: &mut Command) -> Result<(), TileError> {
    let output = command
        .output()
        .map_err(|source| TileError::Spawn { tool, source })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(TileError::Failed {
            tool,
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}
