use std::path::{Path, PathBuf};

use crate::release::{Release, find_superseded};

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum Refused {
    #[error("{0} is not in the bucket. `just releases` lists what it serves.")]
    NotServed(Release),
    #[error("{0} is superseded by {1}, so it is not mirrored.")]
    Superseded(Release, Release),
    #[error("No mirror at {}. Mount its drive, or correct the mirror path.", .0.display())]
    NoMirror(PathBuf),
}

pub fn check_syncable(release: &Release, served: &[Release], mirror: &Path) -> Result<(), Refused> {
    check_verifiable(release, served, mirror)?;
    if let Some(latest) = find_superseded(served).remove(release) {
        return Err(Refused::Superseded(release.clone(), latest));
    }
    Ok(())
}

pub fn check_verifiable(
    release: &Release,
    served: &[Release],
    mirror: &Path,
) -> Result<(), Refused> {
    if !served.contains(release) {
        return Err(Refused::NotServed(release.clone()));
    }
    if !mirror.is_dir() {
        return Err(Refused::NoMirror(mirror.to_path_buf()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(name: &str) -> Release {
        name.parse().expect("a release name")
    }

    fn served() -> Vec<Release> {
        ["2026-08-19.0", "2026-09-23.0", "2026-09-23.1"]
            .into_iter()
            .map(release)
            .collect()
    }

    #[test]
    fn a_current_release_is_syncable_to_an_existing_mirror() {
        let mirror = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            check_syncable(&release("2026-09-23.1"), &served(), mirror.path()),
            Ok(())
        );
    }

    #[test]
    fn a_superseded_release_is_refused() {
        let mirror = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            check_syncable(&release("2026-09-23.0"), &served(), mirror.path()),
            Err(Refused::Superseded(
                release("2026-09-23.0"),
                release("2026-09-23.1")
            ))
        );
    }

    #[test]
    fn a_release_the_bucket_does_not_serve_is_refused() {
        let mirror = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            check_syncable(&release("2099-01-01.0"), &served(), mirror.path()),
            Err(Refused::NotServed(release("2099-01-01.0")))
        );
    }

    #[test]
    fn a_missing_mirror_is_refused() {
        let mirror = tempfile::tempdir().expect("tempdir");
        let unmounted = mirror.path().join("unmounted");

        assert_eq!(
            check_syncable(&release("2026-09-23.1"), &served(), &unmounted),
            Err(Refused::NoMirror(unmounted))
        );
    }

    #[test]
    fn a_superseded_release_is_verifiable() {
        let mirror = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            check_verifiable(&release("2026-09-23.0"), &served(), mirror.path()),
            Ok(())
        );
    }

    #[test]
    fn a_release_the_bucket_does_not_serve_is_not_verifiable() {
        let mirror = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            check_verifiable(&release("2099-01-01.0"), &served(), mirror.path()),
            Err(Refused::NotServed(release("2099-01-01.0")))
        );
    }

    #[test]
    fn a_missing_mirror_is_not_verifiable() {
        let mirror = tempfile::tempdir().expect("tempdir");
        let unmounted = mirror.path().join("unmounted");

        assert_eq!(
            check_verifiable(&release("2026-09-23.1"), &served(), &unmounted),
            Err(Refused::NoMirror(unmounted))
        );
    }
}
