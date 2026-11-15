use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use domain::Span;
use evaluation::cone::{self as stages, ConeSettings, ConeStages, Step};
use itertools::Itertools;
use medallion_model::TrainSessionSamplePositionV3;
use py_common::{runtime_error, table_of_batch, value_error};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pyo3_arrow::PyTable;
use serde::Serialize;

use crate::ScoredSplit;

#[derive(Serialize)]
struct StepRow {
    session_id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    t: DateTime<Utc>,
    lon: f64,
    lat: f64,
    in_split: bool,
    step_metres: Option<f64>,
    repeats_position: bool,
}

#[derive(Serialize)]
struct PairRow {
    session_id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    t: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    t_later: DateTime<Utc>,
}

#[derive(Serialize)]
struct WindowRow {
    session_id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    t: DateTime<Utc>,
    travelled_metres: f64,
    wholly_in_split: bool,
    dwelling: bool,
}

#[derive(Serialize)]
struct PairAngleRow {
    session_id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    t: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    t_later: DateTime<Utc>,
    angle: f64,
}

#[derive(Serialize)]
struct WindowAngleRow {
    session_id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    t: DateTime<Utc>,
    angle: f64,
}

#[derive(Default)]
struct StageRows {
    steps: Vec<StepRow>,
    pairs: Vec<PairRow>,
    windows: Vec<WindowRow>,
    pair_angles: Vec<PairAngleRow>,
    pairs_without_bearing: usize,
    window_angles: Vec<WindowAngleRow>,
}

impl StageRows {
    fn of(session_id: &str, stages: ConeStages) -> Self {
        let id = || session_id.to_string();
        let steps = &stages.steps;
        Self {
            steps: steps
                .iter()
                .map(|step: &Step| StepRow {
                    session_id: id(),
                    t: step.t,
                    lon: step.point.x(),
                    lat: step.point.y(),
                    in_split: step.in_split,
                    step_metres: step.step_metres,
                    repeats_position: step.repeats_position(),
                })
                .collect(),
            pairs: stages
                .pairs
                .iter()
                .map(|pair| PairRow {
                    session_id: id(),
                    t: steps[pair.start].t,
                    t_later: steps[pair.later].t,
                })
                .collect(),
            windows: stages
                .windows
                .iter()
                .map(|window| WindowRow {
                    session_id: id(),
                    t: steps[window.start].t,
                    travelled_metres: window.travelled_metres,
                    wholly_in_split: window.wholly_in_split,
                    dwelling: window.dwelling,
                })
                .collect(),
            pair_angles: stages
                .pair_angles
                .iter()
                .map(|pair| PairAngleRow {
                    session_id: id(),
                    t: steps[pair.start].t,
                    t_later: steps[pair.later].t,
                    angle: pair.degrees,
                })
                .collect(),
            pairs_without_bearing: stages.pairs_without_bearing,
            window_angles: stages
                .window_angles
                .iter()
                .map(|window| WindowAngleRow {
                    session_id: id(),
                    t: window.started_at,
                    angle: window.degrees,
                })
                .collect(),
        }
    }

    fn and(mut self, other: Self) -> Self {
        self.steps.extend(other.steps);
        self.pairs.extend(other.pairs);
        self.windows.extend(other.windows);
        self.pair_angles.extend(other.pair_angles);
        self.pairs_without_bearing += other.pairs_without_bearing;
        self.window_angles.extend(other.window_angles);
        self
    }
}

/// Run the stages that measure how far a train turns from its heading, as a dict of one Arrow
/// table per stage.
///
/// `samples` is a table of `session_id`, `t`, `seq`, `lon`, and `lat`, as silver
/// `train_session_sample_v3` holds them. A window starts at each sample, and reaches every later
/// sample of its session within `horizon`, as text such as `"5m"`. Only the windows that lie wholly
/// in `split`, a `ScoredSplit`, and travel at least `dwelling_metres` are measured.
///
/// The dict's tables are those the evaluation computes, named by stage: `"steps"`, `"pairs"`,
/// `"windows"`, `"pair_angles"`, and `"window_angles"`. Each is keyed by `session_id` and `t`, the
/// instant of a window's start, with `t_later` for a pair's later sample. `"pairs_without_bearing"`
/// counts the measured windows' pairs whose later sample lies at the start's coordinates.
#[pyfunction]
#[pyo3(signature = (*, samples, split, horizon, dwelling_metres))]
pub fn cone_stages<'py>(
    py: Python<'py>,
    samples: PyTable,
    split: PyRef<'py, ScoredSplit>,
    horizon: &str,
    dwelling_metres: f64,
) -> PyResult<Bound<'py, PyDict>> {
    let (batches, _) = samples.into_inner();
    let samples: Vec<TrainSessionSamplePositionV3> =
        medallion::from_batches(&batches).map_err(value_error)?;
    let settings = ConeSettings {
        horizon: Span::new(horizon).map_err(value_error)?.time_delta(),
        dwelling_metres,
    };
    let split = split.split.clone();

    let rows = py.detach(|| {
        sessions_of(samples)
            .iter()
            .map(|(session_id, samples)| {
                stages::cone_stages(samples, &split, &settings)
                    .map(|stages| StageRows::of(session_id, stages))
            })
            .try_fold(StageRows::default(), |rows, session| {
                session.map(|session| rows.and(session))
            })
    });
    let rows = rows.map_err(value_error)?;

    let tables = PyDict::new(py);
    tables.set_item("steps", table_from(&rows.steps, &step_example(), &["t"])?)?;
    tables.set_item(
        "pairs",
        table_from(&rows.pairs, &pair_example(), &["t", "t_later"])?,
    )?;
    tables.set_item(
        "windows",
        table_from(&rows.windows, &window_example(), &["t"])?,
    )?;
    tables.set_item(
        "pair_angles",
        table_from(&rows.pair_angles, &pair_angle_example(), &["t", "t_later"])?,
    )?;
    tables.set_item("pairs_without_bearing", rows.pairs_without_bearing)?;
    tables.set_item(
        "window_angles",
        table_from(&rows.window_angles, &window_angle_example(), &["t"])?,
    )?;
    Ok(tables)
}

fn sessions_of(
    samples: Vec<TrainSessionSamplePositionV3>,
) -> BTreeMap<String, Vec<TrainSessionSamplePositionV3>> {
    samples
        .into_iter()
        .sorted_by_key(|sample| (sample.session_id.to_string(), sample.t, sample.seq))
        .chunk_by(|sample| sample.session_id.to_string())
        .into_iter()
        .map(|(session_id, session)| (session_id, session.collect()))
        .collect()
}

fn table_from<T: Serialize>(rows: &[T], example: &T, instants: &[&str]) -> PyResult<PyTable> {
    let fields = medallion::fields_of(example, instants).map_err(runtime_error)?;
    let batch = serde_arrow::to_record_batch(&fields, &rows).map_err(runtime_error)?;
    table_of_batch(batch)
}

fn step_example() -> StepRow {
    StepRow {
        session_id: String::new(),
        t: DateTime::UNIX_EPOCH,
        lon: 0.0,
        lat: 0.0,
        in_split: true,
        step_metres: Some(0.0),
        repeats_position: false,
    }
}

fn pair_example() -> PairRow {
    PairRow {
        session_id: String::new(),
        t: DateTime::UNIX_EPOCH,
        t_later: DateTime::UNIX_EPOCH,
    }
}

fn window_example() -> WindowRow {
    WindowRow {
        session_id: String::new(),
        t: DateTime::UNIX_EPOCH,
        travelled_metres: 0.0,
        wholly_in_split: true,
        dwelling: false,
    }
}

fn pair_angle_example() -> PairAngleRow {
    PairAngleRow {
        session_id: String::new(),
        t: DateTime::UNIX_EPOCH,
        t_later: DateTime::UNIX_EPOCH,
        angle: 0.0,
    }
}

fn window_angle_example() -> WindowAngleRow {
    WindowAngleRow {
        session_id: String::new(),
        t: DateTime::UNIX_EPOCH,
        angle: 0.0,
    }
}
