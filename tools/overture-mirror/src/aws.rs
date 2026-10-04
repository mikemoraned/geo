use std::path::Path;
use std::process::{Command, ExitStatus};

use crate::listing::{BUCKET, ListingError, PREFIX, served_in};
use crate::release::Release;

#[derive(Debug, thiserror::Error)]
pub enum AwsError {
    #[error("running the AWS CLI: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("the AWS CLI could not list the bucket: {0}")]
    Listing(String),
    #[error(transparent)]
    Unreadable(#[from] ListingError),
    #[error("the copy of {release} stopped with {status}")]
    Copy {
        release: Release,
        status: ExitStatus,
    },
}

pub fn served() -> Result<Vec<Release>, AwsError> {
    let output = aws()
        .args([
            "s3api",
            "list-objects-v2",
            "--bucket",
            BUCKET,
            "--prefix",
            PREFIX,
        ])
        .args(["--delimiter", "/", "--output", "json"])
        .output()?;
    if !output.status.success() {
        return Err(AwsError::Listing(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    Ok(served_in(&String::from_utf8_lossy(&output.stdout))?)
}

pub fn copy(release: &Release, mirror: &Path) -> Result<(), AwsError> {
    let status = aws()
        .args(["s3", "sync"])
        .arg(format!("s3://{BUCKET}/{PREFIX}{release}/"))
        .arg(mirror.join(release.to_string()))
        .status()?;
    if !status.success() {
        return Err(AwsError::Copy {
            release: release.clone(),
            status,
        });
    }
    Ok(())
}

fn aws() -> Command {
    let mut command = Command::new("aws");
    command.arg("--no-sign-request");
    command
}
