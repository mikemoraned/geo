//! Writing the medallion store from python.
//!
//! A derivation prototyped as a notebook still has to produce silver in exactly the form
//! every engine reads: WKB geometry, CRS as PROJJSON, the dataset's own columns, its
//! partition layout, and a rebuild that replaces what it no longer produces. That is one
//! implementation, in `medallion`, and this is the way into it from outside Rust — rather
//! than a second one written in python that has to agree with it.
//!
//! The caller names a dataset and passes a table:
//!
//! ```python
//! import lookout_medallion
//!
//! written = lookout_medallion.write_silver("water_crossing", table)
//! ```
//!
//! `table` is anything exposing the Arrow PyCapsule interface — a pyarrow or DuckDB result,
//! a GeoDataFrame's `to_arrow()` — so the rows are handed over without being copied through
//! python objects. Nothing about the store's layout is stated here: the dataset's definition
//! says which columns it holds and how it is partitioned, and a table that does not match is
//! refused.

use std::path::PathBuf;
use std::sync::OnceLock;

use medallion::{Root, TableError};
use model::TargetError;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3_arrow::PyTable;

/// What a write left in the store.
#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Written {
    /// Rows written, across every partition.
    rows: usize,
    /// Partitions the table covers, and so that were rewritten.
    partitions_written: usize,
    /// Partitions the table no longer covers, and so were deleted.
    partitions_removed: usize,
}

#[pymethods]
impl Written {
    fn __repr__(&self) -> String {
        format!(
            "Written(rows={}, partitions_written={}, partitions_removed={})",
            self.rows, self.partitions_written, self.partitions_removed
        )
    }
}

/// Write `table` as the whole of the silver dataset `dataset`, replacing what is there.
///
/// `root` names the store, defaulting to the one in the repo the caller is working in.
///
/// The table must hold every row of the dataset, since a partition it does not cover is
/// taken to be one the derivation no longer produces, and is deleted.
#[pyfunction]
#[pyo3(signature = (dataset, table, *, root=None))]
fn write_silver(
    py: Python<'_>,
    dataset: &str,
    table: PyTable,
    root: Option<PathBuf>,
) -> PyResult<Written> {
    let target = model::silver_target(dataset).map_err(target_error)?;
    let root = match root {
        Some(path) => Root::new(path),
        None => Root::new(Root::default_path().map_err(|err| PyRuntimeError::new_err(err.to_string()))?),
    };
    let (batches, _) = table.into_inner();

    // The write is filesystem work that calls back into nothing python owns, so the
    // interpreter is left free for the duration.
    let written = py
        .detach(|| runtime().block_on(medallion::write_table(&root, &target, &batches)))
        .map_err(table_error)?;

    Ok(Written {
        rows: written.rows,
        partitions_written: written.partitions.written,
        partitions_removed: written.partitions.removed,
    })
}

/// The runtime the store's async writers run on: one per process, since a call arrives on
/// whichever thread python is on and building a runtime per call would cost more than the
/// write.
fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Runtime::new().expect("a tokio runtime for the store's writers")
    })
}

/// A dataset that cannot be named is the caller's mistake, and is raised as one.
fn target_error(err: TargetError) -> PyErr {
    match err {
        TargetError::NoSuchDataset { .. } => PyValueError::new_err(err.to_string()),
        TargetError::Row(_) => PyRuntimeError::new_err(err.to_string()),
    }
}

/// A table that does not match the dataset is the caller's mistake; anything that goes wrong
/// while writing it is not.
fn table_error(err: TableError) -> PyErr {
    match err {
        TableError::Missing { .. }
        | TableError::Unexpected { .. }
        | TableError::Untranslatable { .. }
        | TableError::UndatedRow { .. }
        | TableError::Country { .. }
        | TableError::UnsupportedLayout { .. }
        | TableError::Unpartitioned(_) => PyValueError::new_err(err.to_string()),
        _ => PyRuntimeError::new_err(err.to_string()),
    }
}

#[pymodule]
fn lookout_medallion(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(write_silver, module)?)?;
    module.add_class::<Written>()?;
    Ok(())
}
