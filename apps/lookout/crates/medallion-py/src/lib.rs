use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use medallion::{Country, Query, QueryError, Root, ScalarValue, TableError, UnknownCountry};
use medallion_model::TargetError;
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
/// The table must hold every row: a partition it does not cover is deleted. `root` defaults to
/// the store in the repo the caller is working in. See `docs/medallion.md`.
#[pyfunction]
#[pyo3(signature = (dataset, table, *, root=None))]
fn write_silver(
    py: Python<'_>,
    dataset: &str,
    table: PyTable,
    root: Option<PathBuf>,
) -> PyResult<Written> {
    let target = medallion_model::silver_target(dataset).map_err(target_error)?;
    let root = root_or_default(root)?;
    let (batches, _) = table.into_inner();

    let written = py
        .detach(|| runtime().block_on(medallion::write_table(&root, &target, &batches)))
        .map_err(table_error)?;

    Ok(Written {
        rows: written.rows,
        partitions_written: written.partitions.written,
        partitions_removed: written.partitions.removed,
    })
}

#[derive(Debug, Clone, FromPyObject)]
enum Param {
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

/// Query the store, returning an Arrow table: the datasets the query names are its tables.
///
/// `country` names the country to read, which a dataset holding one zone per country requires
/// and any other rejects. `params` binds the query's `$name` placeholders as values. The result
/// exposes the Arrow PyCapsule interface, so `pyarrow.table(...)` takes it directly. A dataset
/// the store does not define, or that has never been written, raises a `ValueError` naming it.
/// See `docs/medallion.md`.
#[pyfunction]
#[pyo3(signature = (sql, *, country=None, params=None, root=None))]
fn query_silver(
    py: Python<'_>,
    sql: &str,
    country: Option<&str>,
    params: Option<HashMap<String, Param>>,
    root: Option<PathBuf>,
) -> PyResult<PyTable> {
    let targets = medallion::table_references(sql)
        .map_err(query_error)?
        .iter()
        .map(|dataset| medallion_model::silver_target(dataset).map_err(target_error))
        .collect::<PyResult<Vec<_>>>()?;
    let country = country
        .map(str::parse::<Country>)
        .transpose()
        .map_err(|err: UnknownCountry| PyValueError::new_err(err.to_string()))?;
    let root = root_or_default(root)?;
    let params = params
        .unwrap_or_default()
        .into_iter()
        .map(|(name, param)| (name, ScalarValue::from(param)))
        .collect();

    let batches = py
        .detach(|| {
            runtime().block_on(async {
                let query = Query::new(root);
                for target in &targets {
                    query.register_silver(target, country).await?;
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
/// geometry library takes (`"EPSG:25832"`). See `docs/medallion.md`.
#[pyfunction]
fn projected_crs(country: &str) -> PyResult<String> {
    let country: Country = country
        .parse()
        .map_err(|err: UnknownCountry| PyValueError::new_err(err.to_string()))?;
    Ok(format!("EPSG:{}", country.projected_epsg()))
}

/// The GERS id of the division a country's areas belong to, for matching against the
/// `division_id` column. See `docs/overture.md`.
#[pyfunction]
fn division_id(country: &str) -> PyResult<String> {
    let country: Country = country
        .parse()
        .map_err(|err: UnknownCountry| PyValueError::new_err(err.to_string()))?;
    Ok(medallion_model::division_id(country).to_string())
}

/// The store in the repo the caller is working in, as a path: what the writes here default to.
#[pyfunction]
fn default_root() -> PyResult<PathBuf> {
    Root::default_path().map_err(|err| PyRuntimeError::new_err(err.to_string()))
}

fn root_or_default(root: Option<PathBuf>) -> PyResult<Root> {
    match root {
        Some(path) => Ok(Root::new(path)),
        None => Ok(Root::new(default_root()?)),
    }
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Runtime::new().expect("a tokio runtime for the store's writers")
    })
}

fn query_error(err: QueryError) -> PyErr {
    match err {
        QueryError::NoSuchDataset { .. }
        | QueryError::DataFusion(_)
        | QueryError::UnknownCountry { .. }
        | QueryError::CountryNeeded { .. }
        | QueryError::NoCountryLevel { .. } => PyValueError::new_err(err.to_string()),
        QueryError::Rows(_) | QueryError::Path(_) => PyRuntimeError::new_err(err.to_string()),
    }
}

fn target_error(err: TargetError) -> PyErr {
    match err {
        TargetError::NoSuchDataset { .. } => PyValueError::new_err(err.to_string()),
        TargetError::Row(_) => PyRuntimeError::new_err(err.to_string()),
    }
}

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
    module.add_function(wrap_pyfunction!(division_id, module)?)?;
    module.add_function(wrap_pyfunction!(default_root, module)?)?;
    module.add_class::<Written>()?;
    Ok(())
}
