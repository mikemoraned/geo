//! Writing a batch of rows into the store as one parquet file.

use std::path::Path;

use arrow::array::RecordBatch;
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectPath;
use parquet::arrow::async_writer::ParquetObjectWriter;
use parquet::arrow::AsyncArrowWriter;

/// Failure writing a parquet file into the store.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("{path} is not an absolute path into the store")]
    Path {
        path: String,
        #[source]
        source: object_store::path::Error,
    },
    #[error("object store error: {0}")]
    Store(#[from] object_store::Error),
    #[error("parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    #[error("no batches to write")]
    Empty,
}

/// Write `batches` to `path` as a single parquet file, taking the schema from the first.
///
/// The file appears at `path` only once fully written: the write goes through
/// [`LocalFileSystem`], which stages to a temporary sibling and renames on completion, so
/// an interrupted write leaves nothing a reader can list or open.
pub async fn write_batches(path: &Path, batches: &[RecordBatch]) -> Result<(), WriteError> {
    let Some(first) = batches.first() else {
        return Err(WriteError::Empty);
    };

    let store = LocalFileSystem::new();
    let location = ObjectPath::from_absolute_path(path).map_err(|source| WriteError::Path {
        path: path.display().to_string(),
        source,
    })?;

    let object_writer = ParquetObjectWriter::new(std::sync::Arc::new(store), location);
    let mut writer = AsyncArrowWriter::try_new(object_writer, first.schema(), None)?;
    for batch in batches {
        writer.write(batch).await?;
    }
    writer.close().await?;
    Ok(())
}
