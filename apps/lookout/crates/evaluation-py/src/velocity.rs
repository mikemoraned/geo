use chrono::{DateTime, Utc};
use domain::{Radius, Sample};
use medallion_model::TrainSessionSamplePositionV3;
use predictor::LastMove;
use py_common::value_error;
use pyo3::prelude::*;
use pyo3_arrow::PyTable;
use serde::Serialize;

use crate::cone::{sessions_of, table_from};

#[derive(Serialize)]
struct VelocityRow {
    session_id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    t: DateTime<Utc>,
    speed_mps: Option<f64>,
    heading_degrees: Option<f64>,
}

/// The velocity LightCone estimates at each sample, as an Arrow table.
///
/// `samples` is a table of `session_id`, `t`, `seq`, `lon`, and `lat`, as silver
/// `train_session_sample_v3` holds them. Each row is one sample's `session_id` and `t`, with the
/// `speed_mps` and `heading_degrees` estimated once that sample is observed, from its session's
/// samples so far. A sample within `still_within_metres` of the last one that moved does not
/// move the train. `speed_mps` and `heading_degrees` are null until the train has moved.
#[pyfunction]
#[pyo3(signature = (*, samples, still_within_metres))]
pub fn velocities(samples: PyTable, still_within_metres: f64) -> PyResult<PyTable> {
    let (batches, _) = samples.into_inner();
    let samples: Vec<TrainSessionSamplePositionV3> =
        medallion::from_batches(&batches).map_err(value_error)?;
    let still_within = Radius::new(still_within_metres).map_err(value_error)?;
    let rows = sessions_of(samples)
        .into_iter()
        .map(|(session_id, samples)| {
            let samples = samples
                .iter()
                .map(|sample| Sample::<f64>::at(sample.t, sample.lat, sample.lon))
                .collect::<Result<Vec<_>, _>>()
                .map_err(value_error)?;
            Ok(evaluation::velocities(samples, LastMove::new(still_within))
                .into_iter()
                .map(|at| VelocityRow {
                    session_id: session_id.clone(),
                    t: at.t,
                    speed_mps: at.velocity.map(|velocity| velocity.speed_mps),
                    heading_degrees: at.velocity.map(|velocity| velocity.heading_degrees),
                })
                .collect::<Vec<_>>())
        })
        .collect::<PyResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    table_from(&rows, &velocity_example(), &["t"])
}

fn velocity_example() -> VelocityRow {
    VelocityRow {
        session_id: String::new(),
        t: DateTime::UNIX_EPOCH,
        speed_mps: Some(0.0),
        heading_degrees: Some(0.0),
    }
}
