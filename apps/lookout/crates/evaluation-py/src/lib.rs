use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::OnceLock;

use domain::Percentage;
use evaluation::{Pick, PickError, Seed};
use medallion::{Country, QueryError, Root};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3_arrow::PyTable;

/// Pick `sessions` train sessions of `country` at random under `seed`, as an Arrow table.
///
/// The pick keeps the sessions whose `realtime_percentage` is at least `realtime_at_least`, and
/// returns them in order of `session_id`, with the columns of silver `train_session_v3` other than
/// its geometry. One seed picks the same sessions from the same store on every run. `root`
/// defaults to the store in the repo the caller is working in.
///
/// A `ValueError` is raised for an unknown country, a percentage outside 0 to 100, or a `sessions`
/// of 0. It is also raised when the store holds no train sessions for the country, or fewer that
/// pass the filter than `sessions`.
#[pyfunction]
#[pyo3(signature = (*, country, realtime_at_least, sessions, seed, root=None))]
fn pick(
    py: Python<'_>,
    country: &str,
    realtime_at_least: f64,
    sessions: usize,
    seed: u64,
    root: Option<PathBuf>,
) -> PyResult<PyTable> {
    let asked = Pick {
        country: country.parse::<Country>().map_err(value_error)?,
        realtime_at_least: Percentage::new(realtime_at_least).map_err(value_error)?,
        sessions: NonZeroUsize::new(sessions)
            .ok_or_else(|| PyValueError::new_err("`sessions` must be at least 1"))?,
        seed: Seed(seed),
    };
    let root = root_or_default(root)?;

    let picked = py
        .detach(|| runtime().block_on(evaluation::pick(&root, &asked)))
        .map_err(pick_error)?;

    let batch = medallion::batch(&picked).map_err(runtime_error)?;
    let schema = batch.schema();
    PyTable::try_new(vec![batch], schema).map_err(runtime_error)
}

fn pick_error(err: PickError) -> PyErr {
    match err {
        PickError::TooFew { .. } | PickError::Query(QueryError::NoSuchDataset { .. }) => {
            value_error(err)
        }
        PickError::Query(_) => runtime_error(err),
    }
}

fn root_or_default(root: Option<PathBuf>) -> PyResult<Root> {
    match root {
        Some(path) => Ok(Root::new(path)),
        None => Ok(Root::new(Root::default_path().map_err(runtime_error)?)),
    }
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Runtime::new().expect("a tokio runtime for the store's reads")
    })
}

fn value_error(err: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(err.to_string())
}

fn runtime_error(err: impl std::fmt::Display) -> PyErr {
    PyRuntimeError::new_err(err.to_string())
}

#[pymodule]
fn lookout_evaluation(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(pick, module)?)?;
    Ok(())
}
