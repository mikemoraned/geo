//! The medallion store from python: writing a derivation's silver, and reading any of it back.
//!
//! A derivation prototyped as a notebook still has to produce silver in exactly the form
//! every engine reads: WKB geometry, CRS as PROJJSON, the dataset's own columns, its
//! partition layout, and a rebuild that replaces what it no longer produces. That is one
//! implementation, in `medallion`, and this is the way into it from outside Rust — rather
//! than a second one written in python that has to agree with it. Reading makes the same
//! argument: which files hold a dataset, and what CRS its geometry is in, are the store's to
//! know rather than each reader's.
//!
//! The caller names a dataset, and passes a table or a query:
//!
//! ```python
//! import lookout_medallion
//!
//! written = lookout_medallion.write_silver("water_crossing", table)
//! table = lookout_medallion.query_silver(
//!     "SELECT crossing_id FROM water_crossing WHERE country = $country",
//!     params={"country": "DE"},
//! )
//! ```
//!
//! A table crossing either way is anything exposing the Arrow PyCapsule interface — a pyarrow
//! or DuckDB result, a GeoDataFrame's `to_arrow()` — so the rows are handed over without being
//! copied through python objects. Nothing about the store's layout is stated here: the
//! dataset's definition says which columns it holds and how it is partitioned, and a table
//! that does not match is refused.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use medallion::{Country, Query, QueryError, Root, ScalarValue, TableError, UnknownCountry};
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
    let root = root_or_default(root)?;
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

/// A value bound to a `$name` placeholder in a query. Any other python type is refused.
#[derive(Debug, Clone, FromPyObject)]
enum Param {
    // Ahead of `Int`, since a python `bool` is an `int` and would otherwise bind as one.
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl From<Param> for ScalarValue {
    fn from(param: Param) -> Self {
        match param {
            Param::Bool(value) => Self::Boolean(Some(value)),
            Param::Int(value) => Self::Int64(Some(value)),
            Param::Float(value) => Self::Float64(Some(value)),
            Param::Str(value) => Self::Utf8(Some(value)),
        }
    }
}

/// Query the store, returning an Arrow table.
///
/// The datasets the query reads are the tables it names, each registered under that name with
/// its partitions walked and its geometry columns read back with their CRS.
///
/// `params` binds the query's `$name` placeholders as values, so an id carrying a quote
/// reads as an id that does not exist rather than as more query.
///
/// The result exposes the Arrow PyCapsule interface, so `pyarrow.table(...)` takes it
/// without copying the rows through python objects. A dataset the store does not define, or
/// that has never been written, raises a `ValueError` naming it.
#[pyfunction]
#[pyo3(signature = (sql, *, params=None, root=None))]
fn query_silver(
    py: Python<'_>,
    sql: &str,
    params: Option<HashMap<String, Param>>,
    root: Option<PathBuf>,
) -> PyResult<PyTable> {
    let targets = medallion::table_references(sql)
        .map_err(query_error)?
        .iter()
        .map(|dataset| model::silver_target(dataset).map_err(target_error))
        .collect::<PyResult<Vec<_>>>()?;
    let root = root_or_default(root)?;
    let params = params
        .unwrap_or_default()
        .into_iter()
        .map(|(name, param)| (name, ScalarValue::from(param)))
        .collect();

    // Reading is filesystem work that calls back into nothing python owns, so the
    // interpreter is left free for the duration.
    let batches = py
        .detach(|| {
            runtime().block_on(async {
                let query = Query::new(root);
                for target in &targets {
                    query.register_silver(target).await?;
                }
                query.sql_with_params(sql, params).await
            })
        })
        .map_err(query_error)?;

    // Infallible: a query answers with at least one batch, so the columns are always there.
    let schema = batches
        .first()
        .map(|batch| batch.schema())
        .expect("a query answers with at least one batch");

    PyTable::try_new(batches, schema).map_err(|err| PyRuntimeError::new_err(err.to_string()))
}

/// The CRS a country's projected geometry is stored in, as an authority string a python
/// geometry library takes (`"EPSG:25832"`).
///
/// One zone per country, chosen by the store: a caller preparing the projected column asks
/// rather than naming a zone of its own, so what it projects into and what the file declares
/// cannot disagree.
#[pyfunction]
fn projected_crs(country: &str) -> PyResult<String> {
    let country: Country = country
        .parse()
        .map_err(|err: UnknownCountry| PyValueError::new_err(err.to_string()))?;
    Ok(format!("EPSG:{}", country.projected_epsg()))
}

/// The store in the repo the caller is working in, as a path.
///
/// This is what [`write_silver`] writes into when it is not given a root, so a caller reading
/// the store directly — with duckdb, say — asks for the path rather than working it out, and
/// cannot end up reading one store and writing another.
#[pyfunction]
fn default_root() -> PyResult<PathBuf> {
    Root::default_path().map_err(|err| PyRuntimeError::new_err(err.to_string()))
}

/// The store at `root`, or the one in the repo the caller is working in.
fn root_or_default(root: Option<PathBuf>) -> PyResult<Root> {
    match root {
        Some(path) => Ok(Root::new(path)),
        None => Ok(Root::new(default_root()?)),
    }
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

/// A dataset that has never been written, and a query that does not plan, are both the
/// caller's mistake. What goes wrong reading the files it planned over is not.
fn query_error(err: QueryError) -> PyErr {
    match err {
        QueryError::NoSuchDataset { .. } | QueryError::DataFusion(_) => {
            PyValueError::new_err(err.to_string())
        }
        QueryError::Rows(_) => PyRuntimeError::new_err(err.to_string()),
    }
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
        | TableError::Duplicate { .. }
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
    module.add_function(wrap_pyfunction!(query_silver, module)?)?;
    module.add_function(wrap_pyfunction!(projected_crs, module)?)?;
    module.add_function(wrap_pyfunction!(default_root, module)?)?;
    module.add_class::<Written>()?;
    Ok(())
}
