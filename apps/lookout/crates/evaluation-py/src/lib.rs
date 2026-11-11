use std::num::NonZeroUsize;
use std::path::PathBuf;

use code_version::CodeVersion;
use crossings::silver::ReadError;
use domain::{Percentage, Radius, Span};
use evaluation::gold::{self, GoldError};
use evaluation::record::session_rows;
use evaluation::runs::RunsError;
use evaluation::steps::{self, Predictor, Run, StepError};
use evaluation::{CrowFliesSettings, OracleSettings, Pick, PickError, Score, Seed};
use medallion::Country;
use medallion_model::{
    EvaluationOutcomeRowV1, EvaluationSessionRowV1, EvaluationTransitionRowV1, FloatType,
    TrainSessionRowV3,
};
use py_common::{
    query_error, root_or_default, rows_of, run_error, runtime, runtime_error, table_of, value_error,
};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pyo3_arrow::PyTable;

const BUILD_CODE_VERSION: &str = env!("BUILD_CODE_VERSION");

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

/// CrowFlies, which predicts each crossing within `radius_metres` of the latest sample, and
/// alerts each that arrives within `horizon`, as text such as `"5m"`.
#[pyclass(frozen)]
struct CrowFlies {
    radius: Radius,
    horizon: Span,
}

#[pymethods]
impl CrowFlies {
    #[new]
    #[pyo3(signature = (*, radius_metres, horizon))]
    fn new(radius_metres: f64, horizon: &str) -> PyResult<Self> {
        Ok(Self {
            radius: Radius::new(radius_metres).map_err(value_error)?,
            horizon: span(horizon)?,
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "CrowFlies(radius_metres={:?}, horizon=\"{}\")",
            self.radius.as_metres(),
            self.horizon
        )
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
                    .ok_or_else(|| value_error("`resamples` must be at least 1"))?,
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
    let sessions = sessions_of(sessions)?;
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
    let sessions = sessions_of(sessions)?;
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
    let sessions = sessions_of(sessions)?;
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

/// Run as `run` does, writing each step's table to gold, as a dict of the run's id and the tables.
///
/// The dict's `"run_id"` names the run in gold, and its other keys are those of `run`. The run
/// names `BUILD_CODE_VERSION` and a fingerprint of each silver dataset it reads, as `record` sets
/// out. A `ValueError` is raised if the extension was built from uncommitted code, or if gold
/// already holds the run.
#[pyfunction]
#[pyo3(signature = (
    *, country, realtime_at_least, sessions, seed, predictor, scoring, bootstrap,
    float_type="f32", root=None
))]
#[allow(clippy::too_many_arguments)]
fn evaluate<'py>(
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
    let code = CodeVersion::new(BUILD_CODE_VERSION).map_err(value_error)?;
    let root = root_or_default(root)?;

    let (run_id, ran) = py
        .detach(|| runtime().block_on(gold::run(&root, &settings, &code)))
        .map_err(gold_error)?;

    let tables = PyDict::new(py);
    tables.set_item("run_id", run_id.to_string())?;
    tables.set_item("picked", table_of(&ran.picked)?)?;
    tables.set_item("transitions", table_of(&ran.transitions)?)?;
    tables.set_item("outcomes", table_of(&ran.outcomes)?)?;
    tables.set_item("summary", table_of(&[ran.summary])?)?;
    Ok(tables)
}

/// Record a run over the sessions `picked`, as a dict of its id, its date, and its first two tables.
///
/// The dict's `"run_id"` and `"run_date"` are the text `write_gold` takes. Its `"run"` and
/// `"sessions"` are the run's tables of gold `evaluation_run_v3` and `evaluation_session_v1`. The
/// run names `BUILD_CODE_VERSION`, the code this extension was built from, and a fingerprint of each
/// silver dataset it reads. Its id hashes those with its settings, so the same settings over the
/// same code and data record the same run. If the extension was built from uncommitted code, a
/// `ValueError` is raised.
#[pyfunction]
#[pyo3(signature = (
    *, country, realtime_at_least, sessions, seed, predictor, scoring, bootstrap, picked,
    float_type="f32", root=None
))]
#[allow(clippy::too_many_arguments)]
fn record<'py>(
    py: Python<'py>,
    country: &str,
    realtime_at_least: f64,
    sessions: usize,
    seed: u64,
    predictor: &Bound<'py, PyAny>,
    scoring: PyRef<'py, Scoring>,
    bootstrap: PyRef<'py, Bootstrap>,
    picked: PyTable,
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
    let picked = rows_of::<TrainSessionRowV3>(picked)?;
    let code = CodeVersion::new(BUILD_CODE_VERSION).map_err(value_error)?;
    let root = root_or_default(root)?;

    let recorded = py
        .detach(|| runtime().block_on(evaluation::record::record(&root, &settings, &code, &picked)))
        .map_err(runtime_error)?;

    let tables = PyDict::new(py);
    tables.set_item("run_id", recorded.run_id.to_string())?;
    tables.set_item("run_date", recorded.date.to_string())?;
    tables.set_item("run", table_of(&[recorded.row])?)?;
    tables.set_item("sessions", table_of(&recorded.sessions)?)?;
    Ok(tables)
}

fn picking(country: &str, realtime_at_least: f64, sessions: usize, seed: u64) -> PyResult<Pick> {
    Ok(Pick {
        country: country.parse::<Country>().map_err(value_error)?,
        realtime_at_least: Percentage::new(realtime_at_least).map_err(value_error)?,
        sessions: NonZeroUsize::new(sessions)
            .ok_or_else(|| value_error("`sessions` must be at least 1"))?,
        seed: Seed(seed),
    })
}

fn predictor_of(predictor: &Bound<'_, PyAny>) -> PyResult<Predictor> {
    if let Ok(oracle) = predictor.extract::<PyRef<'_, Oracle>>() {
        return Ok(Predictor::Oracle(OracleSettings {
            horizon: oracle.horizon,
        }));
    }
    if let Ok(crow_flies) = predictor.extract::<PyRef<'_, CrowFlies>>() {
        return Ok(Predictor::CrowFlies(CrowFliesSettings {
            radius: crow_flies.radius,
            horizon: crow_flies.horizon,
            cooldown: None,
        }));
    }
    Err(value_error("`predictor` must be an Oracle or a CrowFlies"))
}

fn float_type_of(float_type: &str) -> PyResult<FloatType> {
    float_type.parse().map_err(|_| {
        value_error(format!(
            "`float_type` must be \"f32\" or \"f64\", not {float_type:?}"
        ))
    })
}

fn span(text: &str) -> PyResult<Span> {
    Span::new(text).map_err(value_error)
}

fn sessions_of(picked: PyTable) -> PyResult<Vec<EvaluationSessionRowV1>> {
    Ok(session_rows(&rows_of::<TrainSessionRowV3>(picked)?))
}

fn pick_error(err: PickError) -> PyErr {
    match err {
        PickError::TooFew { .. } | PickError::NoneEligible => value_error(err),
        PickError::Query(err) => query_error(err),
    }
}

fn step_error(err: StepError) -> PyErr {
    match err {
        StepError::Pick(err) => pick_error(err),
        StepError::Query(err) | StepError::Crossings(ReadError::Query(err)) => query_error(err),
        StepError::Rows(_) => value_error(err),
        _ => runtime_error(err),
    }
}

fn gold_error(err: GoldError) -> PyErr {
    match err {
        GoldError::Pick(err) => pick_error(err),
        GoldError::Step(err) => step_error(err),
        GoldError::Write(err) => run_error(err),
        GoldError::Runs(RunsError::Query(err)) => query_error(err),
        _ => runtime_error(err),
    }
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
    module.add_function(wrap_pyfunction!(evaluate, module)?)?;
    module.add_function(wrap_pyfunction!(record, module)?)?;
    module.add("BUILD_CODE_VERSION", BUILD_CODE_VERSION)?;
    Ok(())
}
