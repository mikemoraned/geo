use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::OnceLock;

use crossings::silver::ReadError;
use domain::{Percentage, Radius, Span};
use evaluation::steps::{self, FloatType, Predictor, Run, StepError};
use evaluation::{Pick, PickError, Score, Seed};
use medallion::{Country, QueryError, Root, Row};
use medallion_model::{EvaluationOutcomeRowV1, EvaluationTransitionRowV1, TrainSessionRowV3};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pyo3_arrow::PyTable;

/// The oracle, which alerts each recorded pass from `horizon` before it, as text such as `"5m"`.
#[pyclass(frozen)]
struct Oracle {
    horizon: Span,
}

#[pymethods]
impl Oracle {
    #[new]
    #[pyo3(signature = (*, horizon))]
    fn new(horizon: &str) -> PyResult<Self> {
        Ok(Self {
            horizon: span(horizon)?,
        })
    }

    fn __repr__(&self) -> String {
        format!("Oracle(horizon=\"{}\")", self.horizon)
    }
}

/// CrowFlies, which predicts each crossing within `radius_metres` of the latest sample.
#[pyclass(frozen)]
struct CrowFlies {
    radius: Radius,
}

#[pymethods]
impl CrowFlies {
    #[new]
    #[pyo3(signature = (*, radius_metres))]
    fn new(radius_metres: f64) -> PyResult<Self> {
        Ok(Self {
            radius: Radius::new(radius_metres).map_err(value_error)?,
        })
    }

    fn __repr__(&self) -> String {
        format!("CrowFlies(radius_metres={:?})", self.radius.as_metres())
    }
}

/// The settings a run scores its notifications by.
///
/// The spans are text such as `"5m"` or `"30s"`, and each score lies from 0 to 1.
#[pyclass(frozen)]
struct Scoring {
    scoring: evaluation::Scoring,
}

#[pymethods]
impl Scoring {
    #[new]
    #[pyo3(signature = (*, horizon, last_moment, ramp, last_moment_score, floor, warm_up))]
    fn new(
        horizon: &str,
        last_moment: &str,
        ramp: &str,
        last_moment_score: f64,
        floor: f64,
        warm_up: &str,
    ) -> PyResult<Self> {
        Ok(Self {
            scoring: evaluation::Scoring {
                horizon: span(horizon)?,
                last_moment: span(last_moment)?,
                ramp: span(ramp)?,
                last_moment_score: Score::new(last_moment_score).map_err(value_error)?,
                floor: Score::new(floor).map_err(value_error)?,
                warm_up: span(warm_up)?,
            },
        })
    }

    fn __repr__(&self) -> String {
        let scoring = &self.scoring;
        format!(
            "Scoring(horizon=\"{}\", last_moment=\"{}\", ramp=\"{}\", last_moment_score={:?}, \
             floor={:?}, warm_up=\"{}\")",
            scoring.horizon,
            scoring.last_moment,
            scoring.ramp,
            scoring.last_moment_score.get(),
            scoring.floor.get(),
            scoring.warm_up,
        )
    }
}

/// The resamples of sessions a summary draws its intervals from, fixed by `seed`.
#[pyclass(frozen)]
struct Bootstrap {
    bootstrap: evaluation::Bootstrap,
}

#[pymethods]
impl Bootstrap {
    #[new]
    #[pyo3(signature = (*, resamples, seed))]
    fn new(resamples: usize, seed: u64) -> PyResult<Self> {
        Ok(Self {
            bootstrap: evaluation::Bootstrap {
                resamples: NonZeroUsize::new(resamples)
                    .ok_or_else(|| PyValueError::new_err("`resamples` must be at least 1"))?,
                seed: Seed(seed),
            },
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "Bootstrap(resamples={}, seed={})",
            self.bootstrap.resamples, self.bootstrap.seed.0
        )
    }
}

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
    let asked = picking(country, realtime_at_least, sessions, seed)?;
    let root = root_or_default(root)?;

    let picked = py
        .detach(|| runtime().block_on(evaluation::pick(&root, &asked)))
        .map_err(pick_error)?;

    table_of(&picked)
}

/// Replay each session in `sessions` through `predictor`, as an Arrow table of transitions.
///
/// `sessions` is a table of `pick`'s columns, and `predictor` an `Oracle` or a `CrowFlies`. The
/// replay runs in `float_type`, `"f32"` or `"f64"`. Each row is one crossing's change of state at
/// one sample, with the columns of gold `evaluation_transition_v1`.
#[pyfunction]
#[pyo3(signature = (*, country, sessions, predictor, float_type="f32", root=None))]
fn replay(
    py: Python<'_>,
    country: &str,
    sessions: PyTable,
    predictor: &Bound<'_, PyAny>,
    float_type: &str,
    root: Option<PathBuf>,
) -> PyResult<PyTable> {
    let country = country.parse::<Country>().map_err(value_error)?;
    let sessions = rows_of::<TrainSessionRowV3>(sessions)?;
    let predictor = predictor_of(predictor)?;
    let float_type = float_type_of(float_type)?;
    let root = root_or_default(root)?;

    let transitions = py
        .detach(|| {
            runtime().block_on(steps::replay(
                &root, country, &sessions, predictor, float_type,
            ))
        })
        .map_err(step_error)?;

    table_of(&transitions)
}

/// Score the notifications in `transitions` against each session's passes, as an Arrow table.
///
/// `transitions` is a table of `replay`'s columns for the sessions in `sessions`. Each row is one
/// notification or one missed pass, with the columns of gold `evaluation_outcome_v1`.
#[pyfunction]
#[pyo3(signature = (*, country, sessions, transitions, scoring, root=None))]
fn score(
    py: Python<'_>,
    country: &str,
    sessions: PyTable,
    transitions: PyTable,
    scoring: PyRef<'_, Scoring>,
    root: Option<PathBuf>,
) -> PyResult<PyTable> {
    let country = country.parse::<Country>().map_err(value_error)?;
    let sessions = rows_of::<TrainSessionRowV3>(sessions)?;
    let transitions = rows_of::<EvaluationTransitionRowV1>(transitions)?;
    let scoring = scoring.scoring;
    let root = root_or_default(root)?;

    let outcomes = py
        .detach(|| {
            runtime().block_on(steps::score(
                &root,
                country,
                &sessions,
                &transitions,
                &scoring,
            ))
        })
        .map_err(step_error)?;

    table_of(&outcomes)
}

/// Sum `outcomes` into the run's measures and their intervals, as a one-row Arrow table.
///
/// `outcomes` is a table of `score`'s columns for the sessions in `sessions`. The row has the
/// columns of gold `evaluation_summary_v1`.
#[pyfunction]
#[pyo3(signature = (*, sessions, outcomes, bootstrap))]
fn summarise(
    sessions: PyTable,
    outcomes: PyTable,
    bootstrap: PyRef<'_, Bootstrap>,
) -> PyResult<PyTable> {
    let sessions = rows_of::<TrainSessionRowV3>(sessions)?;
    let outcomes = rows_of::<EvaluationOutcomeRowV1>(outcomes)?;

    let summary =
        steps::summarise(&sessions, &outcomes, &bootstrap.bootstrap).map_err(step_error)?;

    table_of(&[summary])
}

/// Pick sessions, then replay, score, and summarise them, as a dict of the four steps' tables.
///
/// The dict's keys are `"picked"`, `"transitions"`, `"outcomes"`, and `"summary"`. The arguments
/// are those of the steps.
#[pyfunction]
#[pyo3(signature = (
    *, country, realtime_at_least, sessions, seed, predictor, scoring, bootstrap,
    float_type="f32", root=None
))]
#[allow(clippy::too_many_arguments)]
fn run<'py>(
    py: Python<'py>,
    country: &str,
    realtime_at_least: f64,
    sessions: usize,
    seed: u64,
    predictor: &Bound<'py, PyAny>,
    scoring: PyRef<'py, Scoring>,
    bootstrap: PyRef<'py, Bootstrap>,
    float_type: &str,
    root: Option<PathBuf>,
) -> PyResult<Bound<'py, PyDict>> {
    let settings = Run {
        pick: picking(country, realtime_at_least, sessions, seed)?,
        predictor: predictor_of(predictor)?,
        float_type: float_type_of(float_type)?,
        scoring: scoring.scoring,
        bootstrap: bootstrap.bootstrap,
    };
    let root = root_or_default(root)?;

    let ran = py
        .detach(|| runtime().block_on(steps::run(&root, &settings)))
        .map_err(step_error)?;

    let tables = PyDict::new(py);
    tables.set_item("picked", table_of(&ran.picked)?)?;
    tables.set_item("transitions", table_of(&ran.transitions)?)?;
    tables.set_item("outcomes", table_of(&ran.outcomes)?)?;
    tables.set_item("summary", table_of(&[ran.summary])?)?;
    Ok(tables)
}

fn picking(country: &str, realtime_at_least: f64, sessions: usize, seed: u64) -> PyResult<Pick> {
    Ok(Pick {
        country: country.parse::<Country>().map_err(value_error)?,
        realtime_at_least: Percentage::new(realtime_at_least).map_err(value_error)?,
        sessions: NonZeroUsize::new(sessions)
            .ok_or_else(|| PyValueError::new_err("`sessions` must be at least 1"))?,
        seed: Seed(seed),
    })
}

fn predictor_of(predictor: &Bound<'_, PyAny>) -> PyResult<Predictor> {
    if let Ok(oracle) = predictor.extract::<PyRef<'_, Oracle>>() {
        return Ok(Predictor::Oracle {
            horizon: oracle.horizon,
        });
    }
    if let Ok(crow_flies) = predictor.extract::<PyRef<'_, CrowFlies>>() {
        return Ok(Predictor::CrowFlies {
            radius: crow_flies.radius,
        });
    }
    Err(PyValueError::new_err(
        "`predictor` must be an Oracle or a CrowFlies",
    ))
}

fn float_type_of(float_type: &str) -> PyResult<FloatType> {
    match float_type {
        "f32" => Ok(FloatType::F32),
        "f64" => Ok(FloatType::F64),
        _ => Err(PyValueError::new_err(format!(
            "`float_type` must be \"f32\" or \"f64\", not {float_type:?}"
        ))),
    }
}

fn span(text: &str) -> PyResult<Span> {
    Span::new(text).map_err(value_error)
}

fn rows_of<T: Row>(table: PyTable) -> PyResult<Vec<T>> {
    let (batches, _) = table.into_inner();
    Ok(batches
        .iter()
        .map(medallion::from_batch::<T>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(value_error)?
        .into_iter()
        .flatten()
        .collect())
}

fn table_of<T: Row>(rows: &[T]) -> PyResult<PyTable> {
    let batch = medallion::batch(rows).map_err(runtime_error)?;
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

fn step_error(err: StepError) -> PyErr {
    match err {
        StepError::Pick(err) => pick_error(err),
        StepError::Rows(_)
        | StepError::Query(QueryError::NoSuchDataset { .. })
        | StepError::Crossings(ReadError::Query(QueryError::NoSuchDataset { .. })) => {
            value_error(err)
        }
        _ => runtime_error(err),
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
    module.add_class::<Oracle>()?;
    module.add_class::<CrowFlies>()?;
    module.add_class::<Scoring>()?;
    module.add_class::<Bootstrap>()?;
    module.add_function(wrap_pyfunction!(pick, module)?)?;
    module.add_function(wrap_pyfunction!(replay, module)?)?;
    module.add_function(wrap_pyfunction!(score, module)?)?;
    module.add_function(wrap_pyfunction!(summarise, module)?)?;
    module.add_function(wrap_pyfunction!(run, module)?)?;
    Ok(())
}
