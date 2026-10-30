use std::sync::Arc;

use arrow::array::{Array, ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow::compute::{cast, concat_batches};
use arrow::datatypes::{DataType, Field, FieldRef, TimeUnit};
use chrono::{DateTime, Utc};
use domain::{CrossingId, Pass, Radius, SessionId};
use geo_types::{Geometry, Point};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pyo3_arrow::PyTable;
use serde::{Deserialize, Serialize};
use serde_arrow::schema::{SchemaLike, TracingOptions};
use session_crossings::matching::{self, Crossing, DEFAULT_RADIUS, Near, Sample};

/// The steps the session crossings matching takes for one session, as two tables.
///
/// `samples` holds the session's samples, with `t` and `geometry_projected`, as silver
/// `session_sample_v2` stores them. `crossings` holds the crossings to match against, with
/// `crossing_id`, `geometry`, and `geometry_projected`, as silver `water_crossing_v2` stores them.
/// Either is anything exposing the Arrow PyCapsule interface.
///
/// The answer is a dict of two tables. `near` holds each sample within `radius_metres` of a
/// crossing, with its distance and the number of its visit. `passes` holds one pass per visit,
/// at the visit's nearest sample. A missing or unreadable column raises `ValueError`, as does a
/// `radius_metres` that is not a positive, finite number.
#[pyfunction]
#[pyo3(signature = (session_id, samples, crossings, *, radius_metres=DEFAULT_RADIUS.as_metres()))]
fn steps<'py>(
    py: Python<'py>,
    session_id: &str,
    samples: PyTable,
    crossings: PyTable,
    radius_metres: f64,
) -> PyResult<Bound<'py, PyDict>> {
    let session_id = SessionId::new(session_id).map_err(value_error)?;
    let samples = samples_of(&one_batch(samples)?)?;
    let crossings = crossings_of(&one_batch(crossings)?)?;
    let radius = Radius::new(radius_metres).map_err(value_error)?;

    let mut near: Vec<(CrossingId, Near)> = Vec::new();
    let mut passes = Vec::new();
    for crossing in &crossings {
        let steps =
            matching::steps(&session_id, &samples, crossing, radius).map_err(runtime_error)?;
        near.extend(
            steps
                .near
                .into_iter()
                .map(|one| (crossing.crossing.id.clone(), one)),
        );
        passes.extend(steps.passes);
    }

    let answer = PyDict::new(py);
    answer.set_item("near", table(near_batch(&near)?)?)?;
    answer.set_item("passes", table(passes_batch(&passes)?)?)?;
    Ok(answer)
}

fn one_batch(table: PyTable) -> PyResult<RecordBatch> {
    let (batches, schema) = table.into_inner();
    concat_batches(&schema, &batches).map_err(value_error)
}

fn samples_of(batch: &RecordBatch) -> PyResult<Vec<Sample>> {
    let instants = instants(batch, "t")?;
    let projected = points(batch, "geometry_projected")?;
    let mut samples: Vec<Sample> = instants
        .into_iter()
        .zip(projected)
        .map(|(t, projected)| Sample { t, projected })
        .collect();
    samples.sort_by_key(|sample| sample.t);
    Ok(samples)
}

fn crossings_of(batch: &RecordBatch) -> PyResult<Vec<Crossing>> {
    let ids = strings(batch, "crossing_id")?;
    let positions = points(batch, "geometry")?;
    let projected = points(batch, "geometry_projected")?;
    ids.into_iter()
        .zip(positions)
        .zip(projected)
        .map(|((id, position), projected)| {
            Ok(Crossing {
                crossing: domain::Crossing::new(
                    CrossingId::new(id).map_err(value_error)?,
                    position,
                ),
                projected,
            })
        })
        .collect()
}

fn column<'a>(batch: &'a RecordBatch, name: &str) -> PyResult<&'a ArrayRef> {
    batch
        .column_by_name(name)
        .ok_or_else(|| PyValueError::new_err(format!("the table has no `{name}` column")))
}

fn instants(batch: &RecordBatch, name: &str) -> PyResult<Vec<DateTime<Utc>>> {
    let millis = cast(
        column(batch, name)?,
        &DataType::Timestamp(TimeUnit::Millisecond, None),
    )
    .and_then(|instants| cast(&instants, &DataType::Int64))
    .map_err(|err| unreadable(name, err))?;
    let millis = millis
        .as_any()
        .downcast_ref::<Int64Array>()
        .ok_or_else(|| unreadable(name, "not a timestamp"))?;
    millis
        .iter()
        .map(|milli| {
            milli
                .and_then(DateTime::from_timestamp_millis)
                .ok_or_else(|| unreadable(name, "a missing or out-of-range instant"))
        })
        .collect()
}

fn strings(batch: &RecordBatch, name: &str) -> PyResult<Vec<String>> {
    let text = cast(column(batch, name)?, &DataType::Utf8).map_err(|err| unreadable(name, err))?;
    let text = text
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| unreadable(name, "not text"))?;
    text.iter()
        .map(|value| {
            value
                .map(str::to_string)
                .ok_or_else(|| unreadable(name, "a missing value"))
        })
        .collect()
}

fn points(batch: &RecordBatch, name: &str) -> PyResult<Vec<Point<f64>>> {
    column(batch, name)?;
    medallion::geometries(batch, name)
        .map_err(|err| unreadable(name, err))?
        .into_iter()
        .map(|geometry| match geometry {
            Geometry::Point(point) => Ok(point),
            _ => Err(unreadable(name, "a geometry that is not a point")),
        })
        .collect()
}

#[derive(Debug, Serialize, Deserialize)]
struct NearRow {
    crossing_id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    t: DateTime<Utc>,
    distance_m: f64,
    visit: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct PassRow {
    crossing_id: String,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    crossed_at: DateTime<Utc>,
    distance_m: f64,
    samples_within: u32,
}

fn near_batch(near: &[(CrossingId, Near)]) -> PyResult<RecordBatch> {
    let rows = near
        .iter()
        .map(|(crossing_id, one)| {
            Ok(NearRow {
                crossing_id: crossing_id.to_string(),
                t: one.sample.t,
                distance_m: one.distance_metres,
                visit: one.visit.try_into().map_err(|_| {
                    PyRuntimeError::new_err(format!(
                        "visit {} is more than the `visit` column can number",
                        one.visit
                    ))
                })?,
            })
        })
        .collect::<PyResult<Vec<_>>>()?;
    batch(&rows, "t")
}

fn passes_batch(passes: &[Pass]) -> PyResult<RecordBatch> {
    let rows: Vec<PassRow> = passes
        .iter()
        .map(|pass| PassRow {
            crossing_id: pass.crossing_id.to_string(),
            crossed_at: pass.crossed_at,
            distance_m: pass.distance_metres,
            samples_within: pass.samples_within,
        })
        .collect();
    batch(&rows, "crossed_at")
}

fn batch<T: Serialize + for<'de> Deserialize<'de>>(
    rows: &[T],
    instant: &str,
) -> PyResult<RecordBatch> {
    let fields: Vec<FieldRef> = Vec::<FieldRef>::from_type::<T>(TracingOptions::default())
        .map_err(runtime_error)?
        .into_iter()
        .map(|field| {
            if field.name() == instant {
                Arc::new(Field::new(
                    instant,
                    DataType::Timestamp(TimeUnit::Millisecond, Some("UTC".into())),
                    false,
                ))
            } else {
                field
            }
        })
        .collect();
    serde_arrow::to_record_batch(&fields, &rows).map_err(runtime_error)
}

fn table(batch: RecordBatch) -> PyResult<PyTable> {
    let schema = batch.schema();
    PyTable::try_new(vec![batch], schema).map_err(runtime_error)
}

fn unreadable(name: &str, reason: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(format!("the `{name}` column cannot be read: {reason}"))
}

fn value_error(err: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(err.to_string())
}

fn runtime_error(err: impl std::fmt::Display) -> PyErr {
    PyRuntimeError::new_err(err.to_string())
}

#[pymodule]
fn lookout_session_crossings(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(steps, module)?)?;
    Ok(())
}
