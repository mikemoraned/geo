use std::io;
use std::path::{Path, PathBuf};

use crate::location::Location;
use crate::release::{Release, find_superseded};

#[derive(Debug, Clone, PartialEq, Eq, Hash, thiserror::Error)]
pub enum Refused {
    #[error("{0} is not in the source. `just releases` lists what it holds.")]
    NotServed(Release),
    #[error("{0} is superseded by {1}, so it is not mirrored.")]
    Superseded(Release, Release),
    #[error("No mirror at {}. Mount its drive, or correct the mirror path.", .0.display())]
    NoMirror(PathBuf),
    #[error("The mirror {} is the source itself. Name a different mirror or source.", .0.display())]
    SameAsSource(PathBuf),
    #[error("{} cannot be resolved to a directory: {kind}", path.display())]
    Unresolvable { path: PathBuf, kind: io::ErrorKind },
}

pub fn check_syncable(
    release: &Release,
    served: &[Release],
    source: &Location,
    mirror: &Path,
) -> Result<(), Refused> {
    check_verifiable(release, served, source, mirror)?;
    if let Some(latest) = find_superseded(served).remove(release) {
        return Err(Refused::Superseded(release.clone(), latest));
    }
    Ok(())
}

pub fn check_verifiable(
    release: &Release,
    served: &[Release],
    source: &Location,
    mirror: &Path,
) -> Result<(), Refused> {
    if !mirror.is_dir() {
        return Err(Refused::NoMirror(mirror.to_path_buf()));
    }
    if let Location::Mirror(source) = source
        && same_directory(source, mirror)?
    {
        return Err(Refused::SameAsSource(mirror.to_path_buf()));
    }
    if !served.contains(release) {
        return Err(Refused::NotServed(release.clone()));
    }
    Ok(())
}

fn same_directory(one: &Path, other: &Path) -> Result<bool, Refused> {
    Ok(resolved(one)? == resolved(other)?)
}

fn resolved(path: &Path) -> Result<PathBuf, Refused> {
    path.canonicalize().map_err(|err| Refused::Unresolvable {
        path: path.to_path_buf(),
        kind: err.kind(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::location::OVERTURE;

    fn release(name: &str) -> Release {
        name.parse().expect("a release name")
    }

    fn bucket() -> Location {
        OVERTURE.parse().expect("the Overture bucket")
    }

    fn mirror_at(path: &Path) -> Location {
        Location::Mirror(path.to_path_buf())
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
            check_syncable(
                &release("2026-09-23.1"),
                &served(),
                &bucket(),
                mirror.path()
            ),
            Ok(())
        );
    }

    #[test]
    fn a_superseded_release_is_refused() {
        let mirror = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            check_syncable(
                &release("2026-09-23.0"),
                &served(),
                &bucket(),
                mirror.path()
            ),
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
            check_syncable(
                &release("2099-01-01.0"),
                &served(),
                &bucket(),
                mirror.path()
            ),
            Err(Refused::NotServed(release("2099-01-01.0")))
        );
    }

    #[test]
    fn a_missing_mirror_is_refused() {
        let mirror = tempfile::tempdir().expect("tempdir");
        let unmounted = mirror.path().join("unmounted");

        assert_eq!(
            check_syncable(&release("2026-09-23.1"), &served(), &bucket(), &unmounted),
            Err(Refused::NoMirror(unmounted))
        );
    }

    #[test]
    fn a_superseded_release_is_verifiable() {
        let mirror = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            check_verifiable(
                &release("2026-09-23.0"),
                &served(),
                &bucket(),
                mirror.path()
            ),
            Ok(())
        );
    }

    #[test]
    fn a_release_the_bucket_does_not_serve_is_not_verifiable() {
        let mirror = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            check_verifiable(
                &release("2099-01-01.0"),
                &served(),
                &bucket(),
                mirror.path()
            ),
            Err(Refused::NotServed(release("2099-01-01.0")))
        );
    }

    #[test]
    fn a_missing_mirror_is_not_verifiable() {
        let mirror = tempfile::tempdir().expect("tempdir");
        let unmounted = mirror.path().join("unmounted");

        assert_eq!(
            check_verifiable(&release("2026-09-23.1"), &served(), &bucket(), &unmounted),
            Err(Refused::NoMirror(unmounted))
        );
    }

    #[test]
    fn a_mirror_that_is_its_own_source_is_not_syncable() {
        let mirror = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            check_syncable(
                &release("2026-09-23.1"),
                &served(),
                &mirror_at(mirror.path()),
                mirror.path()
            ),
            Err(Refused::SameAsSource(mirror.path().to_path_buf()))
        );
    }

    #[test]
    fn a_mirror_that_is_its_own_source_is_not_verifiable() {
        let mirror = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            check_verifiable(
                &release("2026-09-23.1"),
                &served(),
                &mirror_at(mirror.path()),
                mirror.path()
            ),
            Err(Refused::SameAsSource(mirror.path().to_path_buf()))
        );
    }

    #[test]
    fn the_same_directory_spelled_another_way_is_its_own_source() {
        let mirror = tempfile::tempdir().expect("tempdir");
        let respelled = mirror.path().join(".");

        assert_eq!(
            check_verifiable(
                &release("2026-09-23.1"),
                &served(),
                &mirror_at(&respelled),
                mirror.path()
            ),
            Err(Refused::SameAsSource(mirror.path().to_path_buf()))
        );
    }

    #[test]
    fn a_mirror_synced_from_another_mirror_is_syncable() {
        let source = tempfile::tempdir().expect("tempdir");
        let mirror = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            check_syncable(
                &release("2026-09-23.1"),
                &served(),
                &mirror_at(source.path()),
                mirror.path()
            ),
            Ok(())
        );
    }

    #[test]
    fn a_mirror_source_that_cannot_be_resolved_is_refused() {
        let mirror = tempfile::tempdir().expect("tempdir");
        let absent = mirror.path().join("absent");

        assert_eq!(
            check_verifiable(
                &release("2026-09-23.1"),
                &served(),
                &mirror_at(&absent),
                mirror.path()
            ),
            Err(Refused::Unresolvable {
                path: absent,
                kind: io::ErrorKind::NotFound,
            })
        );
    }
}
