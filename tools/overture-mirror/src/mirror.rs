use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::{StreamExt, TryStreamExt};
use object_store::aws::AmazonS3Builder;
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectPath;
use object_store::{ObjectMeta, ObjectStore, ObjectStoreExt};
use tokio::io::AsyncWriteExt;

use crate::location::Location;
use crate::progress::{Display, Progress, Theme};
use crate::release::Release;
use crate::signature::Signature;

pub const REGION: &str = "us-west-2";

const CONCURRENT_COPIES: usize = 8;

const PARTIAL: &str = ".part";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Local {
    Complete,
    Missing,
    Differs,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Verification {
    pub remote: BTreeMap<ObjectPath, Local>,
    pub local_only: BTreeSet<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum MirrorError {
    #[error("reading the source: {0}")]
    Store(#[from] object_store::Error),
    #[error("walking the mirror: {0}")]
    Walk(#[from] walkdir::Error),
    #[error("{}: {source}", path.display())]
    Local {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl MirrorError {
    fn local_io(path: &Path) -> impl FnOnce(std::io::Error) -> Self + '_ {
        move |source| Self::Local {
            path: path.to_path_buf(),
            source,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Source {
    store: Arc<dyn ObjectStore>,
    prefix: ObjectPath,
}

impl Source {
    pub fn new(store: Arc<dyn ObjectStore>, prefix: ObjectPath) -> Self {
        Self { store, prefix }
    }

    pub fn open(location: &Location) -> Result<Self, MirrorError> {
        match location {
            Location::Bucket { bucket, prefix } => {
                let store = AmazonS3Builder::new()
                    .with_bucket_name(bucket)
                    .with_region(REGION)
                    .with_skip_signature(true)
                    .build()?;
                Ok(Self::new(Arc::new(store), prefix.clone()))
            }
            Location::Mirror(path) => {
                let store = LocalFileSystem::new_with_prefix(path)?;
                Ok(Self::new(Arc::new(store), ObjectPath::default()))
            }
        }
    }

    pub async fn releases(&self) -> Result<Vec<Release>, MirrorError> {
        let listing = self.store.list_with_delimiter(Some(&self.prefix)).await?;
        let mut releases = listing
            .common_prefixes
            .iter()
            .filter_map(|prefix| Release::new(prefix.filename()?).ok())
            .collect::<Vec<_>>();
        releases.sort();
        Ok(releases)
    }

    async fn objects(&self, release: &Release) -> Result<Vec<ObjectMeta>, MirrorError> {
        let prefix = self.prefix.clone().join(release.to_string());
        Ok(self
            .store
            .list(Some(&prefix))
            .try_filter(|object| {
                futures::future::ready(!object.location.as_ref().ends_with(PARTIAL))
            })
            .try_collect()
            .await?)
    }

    async fn remote_signature(&self, object: &ObjectMeta) -> Result<Signature, MirrorError> {
        let [head, tail] = Signature::ranges(object.size);
        let head = self.store.get_range(&object.location, head).await?;
        let tail = self.store.get_range(&object.location, tail).await?;
        Ok(Signature::new(object.size, head, tail))
    }

    async fn compare_signatures(
        &self,
        object: &ObjectMeta,
        local: &Path,
    ) -> Result<Local, MirrorError> {
        let Some(signature) = Signature::of_file(local).map_err(MirrorError::local_io(local))?
        else {
            return Ok(Local::Missing);
        };
        if signature.size() != object.size || signature != self.remote_signature(object).await? {
            return Ok(Local::Differs);
        }
        Ok(Local::Complete)
    }

    async fn copy(
        &self,
        object: &ObjectMeta,
        local: &Path,
        progress: &Progress,
    ) -> Result<(), MirrorError> {
        let theme = self.theme_of(object);
        let parent = local.parent().unwrap_or(local);
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(MirrorError::local_io(parent))?;
        let mut partial = local.as_os_str().to_owned();
        partial.push(PARTIAL);
        let partial = PathBuf::from(partial);
        let mut file = tokio::fs::File::create(&partial)
            .await
            .map_err(MirrorError::local_io(&partial))?;
        let mut chunks = self.store.get(&object.location).await?.into_stream();
        while let Some(chunk) = chunks.try_next().await? {
            file.write_all(&chunk)
                .await
                .map_err(MirrorError::local_io(&partial))?;
            progress.advance(&theme, chunk.len() as u64);
        }
        file.flush()
            .await
            .map_err(MirrorError::local_io(&partial))?;
        tokio::fs::rename(&partial, local)
            .await
            .map_err(MirrorError::local_io(local))
    }

    async fn ensure(
        &self,
        object: &ObjectMeta,
        mirror: &Path,
        progress: &Progress,
    ) -> Result<Local, MirrorError> {
        let local = self.local_path(mirror, &object.location);
        let state = self.compare_signatures(object, &local).await?;
        match state {
            Local::Complete => progress.skip(&self.theme_of(object), object.size),
            Local::Missing | Local::Differs => self.copy(object, &local, progress).await?,
        }
        Ok(state)
    }

    fn theme_of(&self, object: &ObjectMeta) -> Theme {
        Theme::of(&self.within_prefix(&object.location))
    }

    fn progress_over(&self, objects: &[ObjectMeta], display: Display) -> Progress {
        let files: Vec<(Theme, u64)> = objects
            .iter()
            .map(|object| (self.theme_of(object), object.size))
            .collect();
        Progress::new(&files, display)
    }

    fn within_prefix(&self, location: &ObjectPath) -> ObjectPath {
        location
            .prefix_match(&self.prefix)
            .into_iter()
            .flatten()
            .collect()
    }

    fn local_path(&self, mirror: &Path, location: &ObjectPath) -> PathBuf {
        self.within_prefix(location)
            .parts()
            .fold(mirror.to_path_buf(), |path, part| path.join(part.as_ref()))
    }
}

pub async fn sync(
    source: &Source,
    release: &Release,
    mirror: &Path,
    display: Display,
) -> Result<BTreeMap<ObjectPath, Local>, MirrorError> {
    let objects = source.objects(release).await?;
    let progress = &source.progress_over(&objects, display);
    let found = futures::stream::iter(objects)
        .map(|object| async move {
            let state = source.ensure(&object, mirror, progress).await?;
            Ok((source.within_prefix(&object.location), state))
        })
        .buffer_unordered(CONCURRENT_COPIES)
        .try_collect()
        .await;
    progress.finish();
    found
}

pub async fn verify(
    source: &Source,
    release: &Release,
    mirror: &Path,
    display: Display,
) -> Result<Verification, MirrorError> {
    let objects = source.objects(release).await?;
    let progress = &source.progress_over(&objects, display);
    let expected: BTreeSet<PathBuf> = objects
        .iter()
        .map(|object| source.local_path(mirror, &object.location))
        .collect();
    let remote = futures::stream::iter(objects)
        .map(|object| async move {
            let local = source.local_path(mirror, &object.location);
            let state = source.compare_signatures(&object, &local).await?;
            progress.advance(&source.theme_of(&object), object.size);
            Ok::<_, MirrorError>((source.within_prefix(&object.location), state))
        })
        .buffer_unordered(CONCURRENT_COPIES)
        .try_collect()
        .await;
    progress.finish();
    let remote = remote?;
    let local_only = local_files(&mirror.join(release.to_string()))?
        .into_iter()
        .filter(|path| !expected.contains(path))
        .collect();
    Ok(Verification { remote, local_only })
}

fn local_files(release_dir: &Path) -> Result<Vec<PathBuf>, MirrorError> {
    if !release_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(release_dir) {
        let entry = entry?;
        if entry.file_type().is_file() {
            files.push(entry.into_path());
        }
    }
    Ok(files)
}
