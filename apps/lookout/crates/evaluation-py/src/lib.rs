use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

use arrow::array::{Array, Float64Array, StringArray};

use code_version::CodeVersion;
use crossings::silver::ReadError;
use domain::{HalfWidth, Percentage, Radius, Span};
use evaluation::gold::{self, GoldError};
use evaluation::record::run_session_rows;
use evaluation::runs::RunsError;
use evaluation::steps::{self, EvaluationSettings, Predictor, StepError};
use evaluation::{
    A5Resolution, CrowFliesSettings, LightConeSettings, OnlineDetectorSettings, OracleSettings,
    Pick, PickError, Salt, Score, Seed, Split, TuneBuckets,
};
use geo_types::Point;
use medallion::Country;
use medallion_model::{
    EvaluationEpisodeRowV1, EvaluationOutcomeRowV2, EvaluationOutcomeRowV3, EvaluationSessionRowV3,
    EvaluationTransitionRowV1, FloatType, TrainSessionRowV3,
};
use py_common::{
    query_error, root_or_default, rows_of, run_error, runtime, runtime_error, table_of, value_error,
};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pyo3_arrow::{PyArray, PyTable};

mod cone;
mod velocity;

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
/// alerts each that arrives within `horizon`, as text such as `"5m"`. The speed comes from the
/// train's last move further than `still_within_metres`.
#[pyclass(frozen)]
struct CrowFlies {
    settings: CrowFliesSettings,
}

#[pymethods]
impl CrowFlies {
    #[new]
    #[pyo3(signature = (*, radius_metres, horizon, still_within_metres))]
    fn new(radius_metres: f64, horizon: &str, still_within_metres: f64) -> PyResult<Self> {
        Ok(Self {
            settings: CrowFliesSettings {
                radius: Radius::new(radius_metres).map_err(value_error)?,
                horizon: span(horizon)?,
                still_within: Radius::new(still_within_metres).map_err(value_error)?,
            },
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "CrowFlies(radius_metres={:?}, horizon=\"{}\", still_within_metres={:?})",
            self.settings.radius.as_metres(),
            self.settings.horizon,
            self.settings.still_within.as_metres()
        )
    }
}

/// LightCone, which alerts what CrowFlies alerts, but only within `half_width_degrees` of the
/// train's heading. It takes CrowFlies' `radius_metres`, `horizon`, and `still_within_metres`, and
/// its heading comes from the train's last move further than `still_within_metres`. At a
/// half-width of 180°, it alerts what CrowFlies does.
#[pyclass(frozen)]
struct LightCone {
    settings: LightConeSettings,
}

#[pymethods]
impl LightCone {
    #[new]
    #[pyo3(signature = (*, radius_metres, half_width_degrees, horizon, still_within_metres))]
    fn new(
        radius_metres: f64,
        half_width_degrees: f64,
        horizon: &str,
        still_within_metres: f64,
    ) -> PyResult<Self> {
        Ok(Self {
            settings: LightConeSettings {
                radius: Radius::new(radius_metres).map_err(value_error)?,
                half_width: HalfWidth::new(half_width_degrees).map_err(value_error)?,
                horizon: span(horizon)?,
                still_within: Radius::new(still_within_metres).map_err(value_error)?,
            },
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "LightCone(radius_metres={:?}, half_width_degrees={:?}, horizon=\"{}\", \
             still_within_metres={:?})",
            self.settings.radius.as_metres(),
            self.settings.half_width.as_degrees(),
            self.settings.horizon,
            self.settings.still_within.as_metres()
        )
    }
}

/// The online pass detector, which reports each crossing within `pass_radius_metres` of the latest
/// sample as passing. A run that has one measures how far its passes skew from ground truth.
#[pyclass(frozen)]
struct OnlineDetector {
    settings: OnlineDetectorSettings,
}

#[pymethods]
impl OnlineDetector {
    #[new]
    #[pyo3(signature = (*, pass_radius_metres))]
    fn new(pass_radius_metres: f64) -> PyResult<Self> {
        Ok(Self {
            settings: OnlineDetectorSettings {
                pass_radius: Radius::new(pass_radius_metres).map_err(value_error)?,
            },
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "OnlineDetector(pass_radius_metres={:?})",
            self.settings.pass_radius.as_metres()
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

/// A splitter, which puts each A5 cell at `resolution` in the tune or the test split by the SHA-256
/// of `salt` and the cell. `tune_buckets` of a hundred buckets fall in the tune split.
#[pyclass(frozen)]
struct Splitter {
    splitter: evaluation::Splitter,
}

#[pymethods]
impl Splitter {
    #[new]
    #[pyo3(signature = (*, resolution, salt, tune_buckets))]
    fn new(resolution: u8, salt: &str, tune_buckets: u8) -> PyResult<Self> {
        Ok(Self {
            splitter: evaluation::Splitter {
                resolution: A5Resolution::new(resolution).map_err(value_error)?,
                salt: Salt::new(salt).map_err(value_error)?,
                tune_buckets: TuneBuckets::new(tune_buckets).map_err(value_error)?,
            },
        })
    }

    /// The split of the cell each point lies in, `"tune"` or `"test"`, as an Arrow array of text.
    ///
    /// `lon` and `lat` are Arrow arrays of degrees as float64, of one length and with no nulls. The
    /// split is the one a run gives a crossing at that point.
    #[pyo3(signature = (lon, lat))]
    fn splits(&self, lon: PyArray, lat: PyArray) -> PyResult<PyArray> {
        let (lon, lat) = (degrees(&lon, "lon")?, degrees(&lat, "lat")?);
        if lon.len() != lat.len() {
            return Err(value_error(format!(
                "`lon` holds {} points and `lat` {}",
                lon.len(),
                lat.len()
            )));
        }
        let splits = lon
            .values()
            .iter()
            .zip(lat.values())
            .map(|(&lon, &lat)| {
                self.splitter
                    .cell(Point::new(lon, lat))
                    .map(|cell| self.splitter.split(cell).to_string())
                    .map_err(value_error)
            })
            .collect::<PyResult<Vec<String>>>()?;
        Ok(PyArray::from_array_ref(Arc::new(StringArray::from(splits))))
    }

    fn __repr__(&self) -> String {
        format!(
            "Splitter(resolution={}, salt=\"{}\", tune_buckets={})",
            self.splitter.resolution, self.splitter.salt, self.splitter.tune_buckets
        )
    }
}

fn degrees<'a>(array: &'a PyArray, name: &str) -> PyResult<&'a Float64Array> {
    let degrees = array
        .array()
        .as_any()
        .downcast_ref::<Float64Array>()
        .ok_or_else(|| value_error(format!("`{name}` must be an array of float64")))?;
    if degrees.null_count() > 0 {
        return Err(value_error(format!("`{name}` holds nulls")));
    }
    Ok(degrees)
}

/// The split a run scores, `"tune"` or `"test"`, of the cells `splitter` assigns. A run with a split
/// scores only the crossings in that split's cells.
#[pyclass(frozen)]
struct ScoredSplit {
    split: evaluation::ScoredSplit,
}

#[pymethods]
impl ScoredSplit {
    #[new]
    #[pyo3(signature = (*, splitter, split))]
    fn new(splitter: PyRef<'_, Splitter>, split: &str) -> PyResult<Self> {
        Ok(Self {
            split: evaluation::ScoredSplit {
                splitter: splitter.splitter.clone(),
                split: split.parse::<Split>().map_err(|_| {
                    value_error(format!(
                        "`split` must be \"tune\" or \"test\", not {split:?}"
                    ))
                })?,
            },
        })
    }

    fn __repr__(&self) -> String {
        let splitter = &self.split.splitter;
        format!(
            "ScoredSplit(splitter=Splitter(resolution={}, salt=\"{}\", tune_buckets={}), \
             split=\"{}\")",
            splitter.resolution, splitter.salt, splitter.tune_buckets, self.split.split
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

/// The rows of the sessions `picked` a run replays, as an Arrow table.
///
/// `picked` is a table of `pick`'s columns. Each row has the columns of gold
/// `evaluation_session_v3`: the session's duration, and its scored duration, the time it spends in
/// the cells of `split`, a `ScoredSplit`. Without a split, the scored duration is the duration.
#[pyfunction]
#[pyo3(signature = (*, country, picked, split=None, root=None))]
fn session_rows(
    py: Python<'_>,
    country: &str,
    picked: PyTable,
    split: Option<PyRef<'_, ScoredSplit>>,
    root: Option<PathBuf>,
) -> PyResult<PyTable> {
    let country = country.parse::<Country>().map_err(value_error)?;
    let picked = rows_of::<TrainSessionRowV3>(picked)?;
    let split = split.map(|split| split.split.clone());
    let root = root_or_default(root)?;

    let sessions = py
        .detach(|| runtime().block_on(run_session_rows(&root, country, split.as_ref(), &picked)))
        .map_err(runtime_error)?;

    table_of(&sessions)
}

/// Replay each session in `sessions` through `predictor`, as an Arrow table of transitions.
///
/// `sessions` is a table of `session_rows`' columns, and `predictor` an `Oracle`, a `CrowFlies`,
/// or a `LightCone`.
/// The replay runs in `float_type`, `"f32"` or `"f64"`. Each row is one crossing's change of state
/// at one sample, with the columns of gold `evaluation_transition_v1`. Given `split`, a
/// `ScoredSplit`, the replay first checks the split against the store, and a split that fails a
/// check raises a `ValueError`.
#[pyfunction]
#[pyo3(signature = (*, country, sessions, predictor, float_type="f32", split=None, root=None))]
fn replay(
    py: Python<'_>,
    country: &str,
    sessions: PyTable,
    predictor: &Bound<'_, PyAny>,
    float_type: &str,
    split: Option<PyRef<'_, ScoredSplit>>,
    root: Option<PathBuf>,
) -> PyResult<PyTable> {
    let country = country.parse::<Country>().map_err(value_error)?;
    let sessions = sessions_of(sessions)?;
    let predictor = predictor_of(predictor)?;
    let float_type = float_type_of(float_type)?;
    let split = split.map(|split| split.split.clone());
    let root = root_or_default(root)?;

    let transitions = py
        .detach(|| {
            runtime().block_on(steps::replay(
                &root,
                country,
                &sessions,
                predictor,
                float_type,
                None,
                split.as_ref(),
            ))
        })
        .map_err(step_error)?;

    table_of(&transitions.transitions)
}

/// Score the notifications in `transitions` against each session's passes, as an Arrow table.
///
/// `transitions` is a table of `replay`'s columns for the sessions in `sessions`. Each row is one
/// notification or one missed pass, with the columns of gold `evaluation_outcome_v2`. Given
/// `split`, a `ScoredSplit`, the table holds only the outcomes of crossings in that split, and a
/// split that fails a check against the store raises a `ValueError`.
#[pyfunction]
#[pyo3(signature = (*, country, sessions, transitions, scoring, split=None, root=None))]
fn score(
    py: Python<'_>,
    country: &str,
    sessions: PyTable,
    transitions: PyTable,
    scoring: PyRef<'_, Scoring>,
    split: Option<PyRef<'_, ScoredSplit>>,
    root: Option<PathBuf>,
) -> PyResult<PyTable> {
    let country = country.parse::<Country>().map_err(value_error)?;
    let sessions = sessions_of(sessions)?;
    let transitions = rows_of::<EvaluationTransitionRowV1>(transitions)?;
    let scoring = scoring.scoring;
    let split = split.map(|split| split.split.clone());
    let root = root_or_default(root)?;

    let scored = py
        .detach(|| {
            runtime().block_on(steps::score(
                &root,
                country,
                &sessions,
                &transitions,
                &scoring,
                split.as_ref(),
            ))
        })
        .map_err(step_error)?;

    table_of(&scored.outcomes)
}

/// Sum `outcomes` into the run's measures and their intervals, as a one-row Arrow table.
///
/// `outcomes` is a table of `score`'s columns for the sessions in `sessions`, a table of
/// `session_rows`' columns. False alarms per hour divide by the sessions' scored durations. The row
/// has the columns of gold `evaluation_summary_v3`.
#[pyfunction]
#[pyo3(signature = (*, sessions, outcomes, bootstrap))]
fn summarise(
    sessions: PyTable,
    outcomes: PyTable,
    bootstrap: PyRef<'_, Bootstrap>,
) -> PyResult<PyTable> {
    let sessions = sessions_of(sessions)?;
    let outcomes = rows_of::<EvaluationOutcomeRowV2>(outcomes)?;

    let summary =
        steps::summarise(&sessions, &outcomes, &bootstrap.bootstrap).map_err(step_error)?;

    table_of(&[summary])
}

/// Expand a run's `episodes` into its transitions, as an Arrow table.
///
/// `episodes` is a table of gold `evaluation_episode_v1`'s columns, and `sessions` the run's table of
/// gold `evaluation_session_v3`. Each row of the result is one transition, with the columns of gold
/// `evaluation_transition_v1`, ordered by session, time, and crossing. An episode naming a session
/// index that `sessions` lacks raises a `ValueError`.
#[pyfunction]
#[pyo3(signature = (*, sessions, episodes))]
fn transitions_of(sessions: PyTable, episodes: PyTable) -> PyResult<PyTable> {
    let sessions = sessions_of(sessions)?;
    let episodes = rows_of::<EvaluationEpisodeRowV1>(episodes)?;

    table_of(&evaluation::transition_rows_of(&sessions, &episodes).map_err(value_error)?)
}

/// Rebuild a run's outcomes from their compact rows, as an Arrow table.
///
/// `outcomes` is a table of gold `evaluation_outcome_v3`'s columns, and `sessions` the run's table of
/// gold `evaluation_session_v3`. Each row of the result has the columns of gold
/// `evaluation_outcome_v2`, which `summarise` takes. An outcome naming a session index that
/// `sessions` lacks raises a `ValueError`.
#[pyfunction]
#[pyo3(signature = (*, sessions, outcomes))]
fn outcomes_of(sessions: PyTable, outcomes: PyTable) -> PyResult<PyTable> {
    let sessions = sessions_of(sessions)?;
    let compact = rows_of::<EvaluationOutcomeRowV3>(outcomes)?;

    table_of(&evaluation::outcome_rows_of(&sessions, &compact).map_err(value_error)?)
}

/// Pick sessions, then replay, score, and summarise them, as a dict of the four steps' tables.
///
/// The dict's keys are `"picked"`, `"transitions"`, `"outcomes"`, and `"summary"`. The arguments
/// are those of the steps. If `online_detector` is an `OnlineDetector`, the run also measures how
/// far its passes skew from ground truth, under the key `"skew"`, with the columns of gold
/// `evaluation_skew_v2`. If `split` is a `ScoredSplit`, the run scores only that split, and the
/// counts its checks of the split compared go under the key `"split"`, with the columns of gold
/// `evaluation_split_v1`.
#[pyfunction]
#[pyo3(signature = (
    *, country, realtime_at_least, sessions, seed, predictor, scoring, bootstrap,
    online_detector=None, float_type="f32", split=None, root=None
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
    online_detector: Option<PyRef<'py, OnlineDetector>>,
    float_type: &str,
    split: Option<PyRef<'py, ScoredSplit>>,
    root: Option<PathBuf>,
) -> PyResult<Bound<'py, PyDict>> {
    let settings = EvaluationSettings {
        pick: picking(country, realtime_at_least, sessions, seed)?,
        predictor: predictor_of(predictor)?,
        online_detector: online_detector.map(|online_detector| online_detector.settings),
        float_type: float_type_of(float_type)?,
        scoring: scoring.scoring,
        bootstrap: bootstrap.bootstrap,
        split: split.map(|split| split.split.clone()),
    };
    let root = root_or_default(root)?;

    let results = py
        .detach(|| runtime().block_on(steps::run(&root, &settings)))
        .map_err(step_error)?;

    let tables = PyDict::new(py);
    tables.set_item("picked", table_of(&results.picked)?)?;
    tables.set_item("transitions", table_of(&results.transitions)?)?;
    if let Some(skew) = results.skew {
        tables.set_item("skew", table_of(&[skew])?)?;
    }
    tables.set_item("outcomes", table_of(&results.outcomes)?)?;
    if let Some(split) = results.split {
        tables.set_item("split", table_of(&[split])?)?;
    }
    tables.set_item("summary", table_of(&[results.summary])?)?;
    Ok(tables)
}

/// Run as `run` does, writing each step's table to gold, as a dict of the run's id and the tables.
///
/// The dict's `"run_id"` names the run in gold, and its other keys are those of `run`. Gold holds
/// the transitions as episodes, in `evaluation_episode_v1`, and the outcomes as compact rows, in
/// `evaluation_outcome_v3`: `transitions_of` and `outcomes_of` give back the tables. The run
/// names `BUILD_CODE_VERSION` and a fingerprint of each silver dataset it reads, as `record` sets
/// out. A `ValueError` is raised if the extension was built from uncommitted code, or if gold
/// already holds the run.
#[pyfunction]
#[pyo3(signature = (
    *, country, realtime_at_least, sessions, seed, predictor, scoring, bootstrap,
    online_detector=None, float_type="f32", split=None, root=None
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
    online_detector: Option<PyRef<'py, OnlineDetector>>,
    float_type: &str,
    split: Option<PyRef<'py, ScoredSplit>>,
    root: Option<PathBuf>,
) -> PyResult<Bound<'py, PyDict>> {
    let settings = EvaluationSettings {
        pick: picking(country, realtime_at_least, sessions, seed)?,
        predictor: predictor_of(predictor)?,
        online_detector: online_detector.map(|online_detector| online_detector.settings),
        float_type: float_type_of(float_type)?,
        scoring: scoring.scoring,
        bootstrap: bootstrap.bootstrap,
        split: split.map(|split| split.split.clone()),
    };
    let code = CodeVersion::new(BUILD_CODE_VERSION).map_err(value_error)?;
    let root = root_or_default(root)?;

    let (run_id, results) = py
        .detach(|| runtime().block_on(gold::run(&root, &settings, &code)))
        .map_err(gold_error)?;

    let tables = PyDict::new(py);
    tables.set_item("run_id", run_id.to_string())?;
    tables.set_item("picked", table_of(&results.picked)?)?;
    tables.set_item("transitions", table_of(&results.transitions)?)?;
    if let Some(skew) = results.skew {
        tables.set_item("skew", table_of(&[skew])?)?;
    }
    tables.set_item("outcomes", table_of(&results.outcomes)?)?;
    if let Some(split) = results.split {
        tables.set_item("split", table_of(&[split])?)?;
    }
    tables.set_item("summary", table_of(&[results.summary])?)?;
    Ok(tables)
}

/// Record a run over the sessions `picked`, as a dict of its id, its date, and its first two tables.
///
/// The dict's `"run_id"` and `"run_date"` are the text `write_gold` takes. Its `"run"` and
/// `"sessions"` are the run's tables of gold `evaluation_run_v6` and `evaluation_session_v3`. The
/// run names `BUILD_CODE_VERSION`, the code this extension was built from, and a fingerprint of each
/// silver dataset it reads. Its id hashes those with its settings, `split` among them, so the same
/// settings over the same code and data record the same run. If the extension was built from
/// uncommitted code, a `ValueError` is raised.
#[pyfunction]
#[pyo3(signature = (
    *, country, realtime_at_least, sessions, seed, predictor, scoring, bootstrap, picked,
    online_detector=None, float_type="f32", split=None, root=None
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
    online_detector: Option<PyRef<'py, OnlineDetector>>,
    float_type: &str,
    split: Option<PyRef<'py, ScoredSplit>>,
    root: Option<PathBuf>,
) -> PyResult<Bound<'py, PyDict>> {
    let settings = EvaluationSettings {
        pick: picking(country, realtime_at_least, sessions, seed)?,
        predictor: predictor_of(predictor)?,
        online_detector: online_detector.map(|online_detector| online_detector.settings),
        float_type: float_type_of(float_type)?,
        scoring: scoring.scoring,
        bootstrap: bootstrap.bootstrap,
        split: split.map(|split| split.split.clone()),
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
        return Ok(Predictor::CrowFlies(crow_flies.settings));
    }
    if let Ok(light_cone) = predictor.extract::<PyRef<'_, LightCone>>() {
        return Ok(Predictor::LightCone(light_cone.settings));
    }
    Err(value_error(
        "`predictor` must be an Oracle, a CrowFlies, or a LightCone",
    ))
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

fn sessions_of(sessions: PyTable) -> PyResult<Vec<EvaluationSessionRowV3>> {
    rows_of::<EvaluationSessionRowV3>(sessions)
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
        StepError::Rows(_) | StepError::SplitCheck(_) | StepError::SplitBroken(_) => {
            value_error(err)
        }
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
    module.add_class::<LightCone>()?;
    module.add_class::<OnlineDetector>()?;
    module.add_class::<Scoring>()?;
    module.add_class::<Bootstrap>()?;
    module.add_class::<Splitter>()?;
    module.add_class::<ScoredSplit>()?;
    module.add_function(wrap_pyfunction!(pick, module)?)?;
    module.add_function(wrap_pyfunction!(session_rows, module)?)?;
    module.add_function(wrap_pyfunction!(replay, module)?)?;
    module.add_function(wrap_pyfunction!(score, module)?)?;
    module.add_function(wrap_pyfunction!(summarise, module)?)?;
    module.add_function(wrap_pyfunction!(transitions_of, module)?)?;
    module.add_function(wrap_pyfunction!(outcomes_of, module)?)?;
    module.add_function(wrap_pyfunction!(run, module)?)?;
    module.add_function(wrap_pyfunction!(evaluate, module)?)?;
    module.add_function(wrap_pyfunction!(record, module)?)?;
    module.add_function(wrap_pyfunction!(cone::cone_stages, module)?)?;
    module.add_function(wrap_pyfunction!(velocity::velocities, module)?)?;
    module.add("BUILD_CODE_VERSION", BUILD_CODE_VERSION)?;
    Ok(())
}
