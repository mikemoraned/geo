use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::{StreamExt, TryStreamExt};
use object_store::aws::AmazonS3Builder;
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectPath;
use object_store::{
    BackoffConfig, ClientOptions, ObjectMeta, ObjectStore, ObjectStoreExt, RetryConfig,
};
use tokio::io::{AsyncWriteExt, BufWriter};

use crate::location::Location;
use crate::progress::{Display, Progress};
use crate::release::Release;
use crate::signature::Signature;

const REGION: &str = "us-west-2";

const CONCURRENT_COPIES: usize = 8;

const WRITE_BUFFER: usize = 8 << 20;

const STALL_TIMEOUT: Duration = Duration::from_secs(60);

const MIB: f64 = (1 << 20) as f64;

const PARTIAL: &str = ".part";

const DENIED_NAMES: &[&str] = &[".DS_Store"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalStatus {
    Complete,
    Missing,
    Differs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Comparison {
    status: LocalStatus,
    source_bytes_read: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verification {
    pub remote: BTreeMap<ObjectPath, LocalStatus>,
    pub local_only: BTreeSet<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum MirrorError {
    #[error("reading the source: {0}")]
    Store(#[from] object_store::Error),
    #[error("reading the mirror: {0}")]
    Task(#[from] tokio::task::JoinError),
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

fn patient_retries() -> RetryConfig {
    RetryConfig {
        backoff: BackoffConfig {
            init_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            base: 2.,
        },
        max_retries: 30,
        retry_timeout: Duration::from_secs(30 * 60),
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
                    .with_retry(patient_retries())
                    .with_client_options(
                        ClientOptions::new()
                            .with_timeout_disabled()
                            .with_read_timeout(STALL_TIMEOUT),
                    )
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
                let name = object.location.filename().unwrap_or_default();
                futures::future::ready(!is_denied(name) && !is_partial(name))
            })
            .try_collect()
            .await?)
    }

    async fn remote_signature(&self, object: &ObjectMeta) -> Result<Signature, MirrorError> {
        let ranges = Signature::ranges(object.size);
        let ends = self.store.get_ranges(&object.location, &ranges).await?;
        Ok(Signature::new(object.size, ends.concat()))
    }

    async fn compare_signatures(
        &self,
        object: &ObjectMeta,
        local: &Path,
    ) -> Result<Comparison, MirrorError> {
        let unread = |status| Comparison {
            status,
            source_bytes_read: 0,
        };
        let path = local.to_path_buf();
        let Some(signature) = tokio::task::spawn_blocking(move || Signature::of_file(&path))
            .await?
            .map_err(MirrorError::local_io(local))?
        else {
            return Ok(unread(LocalStatus::Missing));
        };
        if signature.size() != object.size {
            return Ok(unread(LocalStatus::Differs));
        }
        let status = if signature == self.remote_signature(object).await? {
            LocalStatus::Complete
        } else {
            LocalStatus::Differs
        };
        Ok(Comparison {
            status,
            source_bytes_read: Signature::read_size(object.size),
        })
    }

    async fn copy(
        &self,
        object: &ObjectMeta,
        local: &Path,
        progress: &Progress,
    ) -> Result<(), MirrorError> {
        let parent = local.parent().unwrap_or(local);
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(MirrorError::local_io(parent))?;
        let mut partial = local.as_os_str().to_owned();
        partial.push(PARTIAL);
        let partial = PathBuf::from(partial);
        let file = tokio::fs::File::create(&partial)
            .await
            .map_err(MirrorError::local_io(&partial))?;
        let mut file = BufWriter::with_capacity(WRITE_BUFFER, file);
        let started = std::time::Instant::now();
        let mut chunks = self.store.get(&object.location).await?.into_stream();
        while let Some(chunk) = chunks.try_next().await? {
            file.write_all(&chunk)
                .await
                .map_err(MirrorError::local_io(&partial))?;
            progress.advance(chunk.len() as u64);
        }
        file.flush()
            .await
            .map_err(MirrorError::local_io(&partial))?;
        let seconds = started.elapsed().as_secs_f64();
        tracing::info!(
            "copied {} ({:.1} MiB) in {seconds:.1}s, {:.1} MiB/s",
            object.location,
            object.size as f64 / MIB,
            object.size as f64 / MIB / seconds.max(f64::EPSILON),
        );
        tokio::fs::rename(&partial, local)
            .await
            .map_err(MirrorError::local_io(local))
    }

    async fn ensure(
        &self,
        object: &ObjectMeta,
        mirror: &Path,
        progress: &Progress,
    ) -> Result<LocalStatus, MirrorError> {
        let local = self.local_path(mirror, &object.location);
        let state = self.compare_signatures(object, &local).await?.status;
        let reason = match state {
            LocalStatus::Complete => {
                tracing::info!(
                    "skipping {} ({:.1} MiB): complete in the mirror",
                    object.location,
                    object.size as f64 / MIB,
                );
                progress.skip(object.size);
                return Ok(state);
            }
            LocalStatus::Missing => "missing from the mirror",
            LocalStatus::Differs => "differs from the source",
        };
        tracing::info!(
            "copying {} ({:.1} MiB): {reason}",
            object.location,
            object.size as f64 / MIB,
        );
        self.copy(object, &local, progress).await?;
        Ok(state)
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
) -> Result<BTreeMap<ObjectPath, LocalStatus>, MirrorError> {
    let objects = source.objects(release).await?;
    let progress = &Progress::new(objects.iter().map(|object| object.size).sum(), display);
    let found = futures::stream::iter(objects)
        .map(|object| async move {
            let state = source.ensure(&object, mirror, progress).await?;
            Ok((source.within_prefix(&object.location), state))
        })
        .buffer_unordered(CONCURRENT_COPIES)
        .try_collect()
        .await;
    progress.close(found)
}

pub async fn verify(
    source: &Source,
    release: &Release,
    mirror: &Path,
    display: Display,
) -> Result<Verification, MirrorError> {
    let objects = source.objects(release).await?;
    let progress = &Progress::new(
        objects
            .iter()
            .map(|object| Signature::read_size(object.size))
            .sum(),
        display,
    );
    let objects: Vec<(ObjectMeta, PathBuf)> = objects
        .into_iter()
        .map(|object| {
            let local = source.local_path(mirror, &object.location);
            (object, local)
        })
        .collect();
    let expected: BTreeSet<PathBuf> = objects.iter().map(|(_, local)| local.clone()).collect();
    let remote = futures::stream::iter(objects)
        .map(|(object, local)| async move {
            let comparison = source.compare_signatures(&object, &local).await?;
            tracing::info!("checked {}: {:?}", object.location, comparison.status);
            progress.advance(comparison.source_bytes_read);
            progress.skip(Signature::read_size(object.size) - comparison.source_bytes_read);
            Ok::<_, MirrorError>((source.within_prefix(&object.location), comparison.status))
        })
        .buffer_unordered(CONCURRENT_COPIES)
        .try_collect()
        .await;
    let remote = progress.close(remote)?;
    let local_only = local_files(&mirror.join(release.to_string()))?
        .into_iter()
        .filter(|path| !expected.contains(path))
        .collect();
    Ok(Verification { remote, local_only })
}

fn is_denied(name: &str) -> bool {
    DENIED_NAMES.contains(&name)
}

fn is_partial(name: &str) -> bool {
    name.ends_with(PARTIAL)
}

fn local_files(release_dir: &Path) -> Result<Vec<PathBuf>, MirrorError> {
    if !release_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(release_dir) {
        let entry = entry?;
        if entry.file_type().is_file() && !entry.file_name().to_str().is_some_and(is_denied) {
            files.push(entry.into_path());
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use object_store::PutPayload;
    use object_store::memory::InMemory;

    use super::*;

    const NAME: &str = "2026-09-23.1/theme=base/type=water/part-0.parquet";

    async fn source_holding(bytes: &[u8]) -> (Source, ObjectMeta) {
        let store = InMemory::new();
        let location = ObjectPath::from(NAME);
        store
            .put(&location, PutPayload::from(bytes.to_vec()))
            .await
            .expect("put an object");
        let object = store.head(&location).await.expect("the object's metadata");
        (Source::new(Arc::new(store), ObjectPath::default()), object)
    }

    async fn compared_with(local: Option<&[u8]>) -> Comparison {
        let (source, object) = source_holding(&[7; 5000]).await;
        let mirror = tempfile::tempdir().expect("tempdir");
        let path = source.local_path(mirror.path(), &object.location);
        if let Some(bytes) = local {
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("dirs");
            std::fs::write(&path, bytes).expect("write the local file");
        }
        source
            .compare_signatures(&object, &path)
            .await
            .expect("compare")
    }

    #[tokio::test]
    async fn a_missing_file_reads_nothing_from_the_source() {
        assert_eq!(
            compared_with(None).await,
            Comparison {
                status: LocalStatus::Missing,
                source_bytes_read: 0,
            }
        );
    }

    #[tokio::test]
    async fn a_file_of_another_size_reads_nothing_from_the_source() {
        assert_eq!(
            compared_with(Some(&[7; 10])).await,
            Comparison {
                status: LocalStatus::Differs,
                source_bytes_read: 0,
            }
        );
    }

    #[tokio::test]
    async fn a_file_of_the_same_size_reads_its_signature_from_the_source() {
        assert_eq!(
            compared_with(Some(&[7; 5000])).await,
            Comparison {
                status: LocalStatus::Complete,
                source_bytes_read: 2048,
            }
        );
    }
}
