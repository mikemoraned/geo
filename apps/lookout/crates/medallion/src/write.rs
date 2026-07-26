//! Writing a batch of rows into the store as one parquet file.

use std::fs::File;
use std::path::Path;

use arrow::array::RecordBatch;
use parquet::arrow::ArrowWriter;

/// Failure writing a parquet file into the store.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    #[error("no batches to write")]
    Empty,
}

/// Write `batches` to `path` as a single parquet file, creating parent directories.
///
/// The file appears at `path` only once fully written: it is staged alongside and renamed,
/// so a failed or interrupted write leaves no partial file for a reader to trip over.
pub fn write_batches(path: &Path, batches: &[RecordBatch]) -> Result<(), WriteError> {
    let Some(first) = batches.first() else {
        return Err(WriteError::Empty);
    };

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|source| WriteError::Io {
            path: dir.display().to_string(),
            source,
        })?;
    }

    let staged = staging_path(path);
    let result = write_all(&staged, first.schema(), batches);
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
        return result;
    }

    std::fs::rename(&staged, path).map_err(|source| WriteError::Io {
        path: path.display().to_string(),
        source,
    })
}

fn write_all(
    staged: &Path,
    schema: arrow::datatypes::SchemaRef,
    batches: &[RecordBatch],
) -> Result<(), WriteError> {
    let file = File::create(staged).map_err(|source| WriteError::Io {
        path: staged.display().to_string(),
        source,
    })?;
    let mut writer = ArrowWriter::try_new(file, schema, None)?;
    for batch in batches {
        writer.write(batch)?;
    }
    writer.close()?;
    Ok(())
}

/// A hidden sibling of the destination, so staging shares its filesystem (rename is then
/// atomic) and is skipped by the `*.parquet` globs readers use.
fn staging_path(path: &Path) -> std::path::PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{name}.staging"))
}
