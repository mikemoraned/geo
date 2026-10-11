use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use object_store::memory::InMemory;
use object_store::path::Path as ObjectPath;
use object_store::{ObjectStoreExt, PutPayload};
use overture_mirror::location::Location;
use overture_mirror::mirror::{LocalStatus, Source, Verification, sync, verify};
use overture_mirror::progress::Display;
use overture_mirror::release::Release;

const WATER: &str = "release/2026-09-23.1/theme=base/type=water/part-0.parquet";
const SEGMENT: &str = "release/2026-09-23.1/theme=transportation/type=segment/part-0.parquet";
const DIVISION: &str = "release/2026-09-23.1/theme=divisions/type=division/part-0.parquet";
const SUPERSEDED_WATER: &str = "release/2026-09-23.0/theme=base/type=water/part-0.parquet";

fn contents(seed: u8) -> Vec<u8> {
    (0..5000u32).map(|i| (i as u8).wrapping_mul(seed)).collect()
}

async fn bucket() -> Source {
    let store = InMemory::new();
    for (seed, name) in [
        (3, WATER),
        (5, SEGMENT),
        (7, DIVISION),
        (1, SUPERSEDED_WATER),
    ] {
        store
            .put(&ObjectPath::from(name), PutPayload::from(contents(seed)))
            .await
            .expect("put an object");
    }
    Source::new(Arc::new(store), ObjectPath::from("release"))
}

fn release() -> Release {
    Release::new("2026-09-23.1").expect("a release")
}

fn within_release_prefix(name: &str) -> ObjectPath {
    ObjectPath::from(name.trim_start_matches("release/"))
}

fn local(mirror: &Path, name: &str) -> std::path::PathBuf {
    mirror.join(within_release_prefix(name).as_ref())
}

async fn synced() -> (Source, TempDir) {
    let bucket = bucket().await;
    let mirror = tempfile::tempdir().expect("tempdir");
    sync(&bucket, &release(), mirror.path(), Display::Hidden)
        .await
        .expect("sync");
    (bucket, mirror)
}

fn all(status: LocalStatus) -> BTreeMap<ObjectPath, LocalStatus> {
    [WATER, SEGMENT, DIVISION]
        .into_iter()
        .map(|name| (within_release_prefix(name), status))
        .collect()
}

#[tokio::test]
async fn the_releases_are_the_prefixes_under_release_in_order() {
    let releases = bucket().await.releases().await.expect("the releases");

    assert_eq!(
        releases,
        vec![
            Release::new("2026-09-23.0").expect("a release"),
            Release::new("2026-09-23.1").expect("a release"),
        ]
    );
}

#[tokio::test]
async fn a_first_sync_copies_every_object_of_the_release() {
    let bucket = bucket().await;
    let mirror = tempfile::tempdir().expect("tempdir");

    let found = sync(&bucket, &release(), mirror.path(), Display::Hidden)
        .await
        .expect("sync");

    assert_eq!(found, all(LocalStatus::Missing));
    assert_eq!(
        std::fs::read(local(mirror.path(), SEGMENT)).expect("read"),
        contents(5)
    );
}

#[tokio::test]
async fn a_second_sync_finds_every_object_complete() {
    let (bucket, mirror) = synced().await;

    let found = sync(&bucket, &release(), mirror.path(), Display::Hidden)
        .await
        .expect("sync");

    assert_eq!(found, all(LocalStatus::Complete));
}

#[tokio::test]
async fn a_resumed_sync_recopies_only_the_damaged_and_missing_objects() {
    let (bucket, mirror) = synced().await;
    let mut damaged = contents(3);
    damaged[4999] ^= 0xff;
    std::fs::write(local(mirror.path(), WATER), damaged).expect("damage one");
    std::fs::write(local(mirror.path(), SEGMENT), &contents(5)[..100]).expect("truncate one");
    std::fs::remove_file(local(mirror.path(), DIVISION)).expect("remove one");

    let found = sync(&bucket, &release(), mirror.path(), Display::Hidden)
        .await
        .expect("sync");

    assert_eq!(
        found,
        BTreeMap::from([
            (within_release_prefix(WATER), LocalStatus::Differs),
            (within_release_prefix(SEGMENT), LocalStatus::Differs),
            (within_release_prefix(DIVISION), LocalStatus::Missing),
        ])
    );
    for (seed, name) in [(3, WATER), (5, SEGMENT), (7, DIVISION)] {
        assert_eq!(
            std::fs::read(local(mirror.path(), name)).expect("read"),
            contents(seed)
        );
    }
}

#[tokio::test]
async fn a_synced_release_verifies_complete() {
    let (bucket, mirror) = synced().await;

    let verification = verify(&bucket, &release(), mirror.path(), Display::Hidden)
        .await
        .expect("verify");

    assert_eq!(
        verification,
        Verification {
            remote: all(LocalStatus::Complete),
            local_only: BTreeSet::new(),
        }
    );
}

#[tokio::test]
async fn verify_reports_damaged_missing_and_local_only_files_and_copies_nothing() {
    let (bucket, mirror) = synced().await;
    let damaged = vec![0u8; 5000];
    std::fs::write(local(mirror.path(), WATER), &damaged).expect("damage one");
    std::fs::remove_file(local(mirror.path(), DIVISION)).expect("remove one");
    let leftover = local(mirror.path(), &format!("{SEGMENT}.part"));
    std::fs::write(&leftover, b"partial").expect("leave a partial copy");

    let verification = verify(&bucket, &release(), mirror.path(), Display::Hidden)
        .await
        .expect("verify");

    assert_eq!(
        verification,
        Verification {
            remote: BTreeMap::from([
                (within_release_prefix(WATER), LocalStatus::Differs),
                (within_release_prefix(SEGMENT), LocalStatus::Complete),
                (within_release_prefix(DIVISION), LocalStatus::Missing),
            ]),
            local_only: BTreeSet::from([leftover]),
        }
    );
    assert_eq!(
        std::fs::read(local(mirror.path(), WATER)).expect("read"),
        damaged
    );
}

fn mirror_source(mirror: &Path) -> Source {
    Source::open(&Location::Mirror(mirror.to_path_buf())).expect("a mirror source")
}

#[tokio::test]
async fn a_mirror_synced_from_another_verifies_against_it() {
    let (_, portable) = synced().await;
    let nas = tempfile::tempdir().expect("tempdir");

    sync(
        &mirror_source(portable.path()),
        &release(),
        nas.path(),
        Display::Hidden,
    )
    .await
    .expect("sync");
    let verification = verify(
        &mirror_source(portable.path()),
        &release(),
        nas.path(),
        Display::Hidden,
    )
    .await
    .expect("verify");

    assert_eq!(
        verification
            .remote
            .values()
            .filter(|state| **state == LocalStatus::Complete)
            .count(),
        3
    );
    assert!(verification.local_only.is_empty());
    assert_eq!(
        std::fs::read(local(nas.path(), WATER)).expect("read"),
        contents(3)
    );
}

#[tokio::test]
async fn a_mirror_lists_its_releases_and_skips_other_directories() {
    let (_, portable) = synced().await;
    std::fs::create_dir(portable.path().join("@eaDir")).expect("a directory the NAS adds");

    let releases = mirror_source(portable.path())
        .releases()
        .await
        .expect("the releases");

    assert_eq!(releases, vec![release()]);
}

#[tokio::test]
async fn a_partial_copy_in_the_source_is_not_synced() {
    let (_, portable) = synced().await;
    let nas = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        local(portable.path(), &format!("{SEGMENT}.part")),
        b"partial",
    )
    .expect("leave a partial copy");

    let found = sync(
        &mirror_source(portable.path()),
        &release(),
        nas.path(),
        Display::Hidden,
    )
    .await
    .expect("sync");

    assert_eq!(found, all(LocalStatus::Missing));
    assert!(!local(nas.path(), &format!("{SEGMENT}.part")).exists());
}

#[tokio::test]
async fn a_denied_file_in_the_source_is_not_synced() {
    let (_, portable) = synced().await;
    let nas = tempfile::tempdir().expect("tempdir");
    let finder = portable.path().join("2026-09-23.1/theme=base/.DS_Store");
    std::fs::write(&finder, b"finder state").expect("leave a .DS_Store");

    let found = sync(
        &mirror_source(portable.path()),
        &release(),
        nas.path(),
        Display::Hidden,
    )
    .await
    .expect("sync");

    assert_eq!(found, all(LocalStatus::Missing));
    assert!(
        !nas.path()
            .join("2026-09-23.1/theme=base/.DS_Store")
            .exists()
    );
}

#[tokio::test]
async fn a_denied_file_in_the_mirror_is_not_reported_as_local_only() {
    let (bucket, mirror) = synced().await;
    std::fs::write(
        mirror.path().join("2026-09-23.1/theme=base/.DS_Store"),
        b"finder state",
    )
    .expect("leave a .DS_Store");

    let verification = verify(&bucket, &release(), mirror.path(), Display::Hidden)
        .await
        .expect("verify");

    assert!(verification.local_only.is_empty());
}
