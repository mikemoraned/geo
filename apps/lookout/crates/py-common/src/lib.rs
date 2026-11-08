use std::path::PathBuf;
use std::sync::OnceLock;

use arrow::array::RecordBatch;
use medallion::{QueryError, Root, Row, RunError, TableError};
use medallion_model::{GoldTargetError, TargetError};
use pyo3::PyErr;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3_arrow::PyTable;

pub fn value_error(err: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(err.to_string())
}

pub fn runtime_error(err: impl std::fmt::Display) -> PyErr {
    PyRuntimeError::new_err(err.to_string())
}

pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    // Infallible in practice: building a multi-threaded runtime fails only when the process
    // cannot create threads, and then no store call could run.
    RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("a tokio runtime for the store"))
}

pub fn root_or_default(root: Option<PathBuf>) -> Result<Root, PyErr> {
    match root {
        Some(path) => Ok(Root::new(path)),
        None => Ok(Root::new(Root::default_path().map_err(runtime_error)?)),
    }
}

pub fn rows_of<T: Row>(table: PyTable) -> Result<Vec<T>, PyErr> {
    let (batches, _) = table.into_inner();
    medallion::from_batches::<T>(&batches).map_err(value_error)
}

pub fn table_of<T: Row>(rows: &[T]) -> Result<PyTable, PyErr> {
    table_of_batch(medallion::batch(rows).map_err(runtime_error)?)
}

pub fn table_of_batch(batch: RecordBatch) -> Result<PyTable, PyErr> {
    let schema = batch.schema();
    PyTable::try_new(vec![batch], schema).map_err(runtime_error)
}

pub fn query_error(err: QueryError) -> PyErr {
    match err {
        QueryError::NoSuchDataset { .. }
        | QueryError::DataFusion(_)
        | QueryError::UnknownCountry { .. }
        | QueryError::CountryNeeded { .. }
        | QueryError::NoCountryLevel { .. } => value_error(err),
        QueryError::Rows(_)
        | QueryError::Describing(_)
        | QueryError::Path(_)
        | QueryError::List { .. } => runtime_error(err),
    }
}

pub fn table_error(err: TableError) -> PyErr {
    match err {
        TableError::Missing { .. }
        | TableError::Unexpected { .. }
        | TableError::Duplicate { .. }
        | TableError::Untranslatable { .. }
        | TableError::UndatedRow { .. }
        | TableError::Country { .. }
        | TableError::UnsupportedLayout { .. }
        | TableError::Unpartitioned(_) => value_error(err),
        _ => runtime_error(err),
    }
}

pub fn run_error(err: RunError) -> PyErr {
    match err {
        RunError::Written { .. } | RunError::NotByRun { .. } | RunError::Geometry { .. } => {
            value_error(err)
        }
        RunError::Table(err) => table_error(err),
        RunError::Path(_) | RunError::Row(_) | RunError::Write(_) => runtime_error(err),
    }
}

pub fn target_error(err: TargetError) -> PyErr {
    match err {
        TargetError::NoSuchDataset { .. } => value_error(err),
        TargetError::Row(_) => runtime_error(err),
    }
}

pub fn gold_target_error(err: GoldTargetError) -> PyErr {
    match err {
        GoldTargetError::NoSuchDataset { .. } => value_error(err),
        GoldTargetError::Row(_) => runtime_error(err),
    }
}
