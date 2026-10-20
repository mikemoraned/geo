use std::sync::Arc;

use arrow::array::{
    Array, ArrayRef, Float64Array, Int64Array, RecordBatch, StringArray, TimestampMillisecondArray,
    UInt32Array,
};
use arrow::compute::{cast, concat_batches};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use chrono::{DateTime, Utc};
use domain::{CrossingId, Radius, SessionId};
use geo_types::{Geometry, Point};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pyo3_arrow::PyTable;
use session_crossings::matching::{self, Crossing, DEFAULT_RADIUS, Near, Sample};

/// The steps the session crossings matching takes for one session, as two tables.
///
/// `samples` holds the session's samples, with `t` and `geometry_projected`, as silver
/// `session_sample` stores them. `crossings` holds the crossings to match against, with
/// `crossing_id`, `geometry`, and `geometry_projected`, as silver `water_crossing` stores them.
/// Either is anything exposing the Arrow PyCapsule interface.
///
/// The answer is a dict of two tables. `near` holds each sample within `radius_metres` of a
/// crossing, with its distance and the number of its visit. `passes` holds one pass per visit,
/// at the visit's nearest sample. A missing or unreadable column raises `ValueError`.
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
    let radius = Radius::new(radius_metres);

    let mut near: Vec<(CrossingId, Near)> = Vec::new();
    let mut passes = Vec::new();
    for crossing in &crossings {
        let steps = matching::steps(&session_id, &samples, crossing, radius);
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

fn near_batch(near: &[(CrossingId, Near)]) -> PyResult<RecordBatch> {
    batch(vec![
        (
            "crossing_id",
            Arc::new(StringArray::from_iter_values(
                near.iter().map(|(id, _)| id.as_str()),
            )) as ArrayRef,
        ),
        (
            "t",
            Arc::new(instant_array(near.iter().map(|(_, one)| one.sample.t))),
        ),
        (
            "distance_m",
            Arc::new(Float64Array::from_iter_values(
                near.iter().map(|(_, one)| one.distance_metres),
            )),
        ),
        (
            "visit",
            Arc::new(UInt32Array::from_iter_values(
                near.iter()
                    .map(|(_, one)| one.visit.try_into().unwrap_or(u32::MAX)),
            )),
        ),
    ])
}

fn passes_batch(passes: &[domain::Pass]) -> PyResult<RecordBatch> {
    batch(vec![
        (
            "crossing_id",
            Arc::new(StringArray::from_iter_values(
                passes.iter().map(|pass| pass.crossing_id.as_str()),
            )) as ArrayRef,
        ),
        (
            "crossed_at",
            Arc::new(instant_array(passes.iter().map(|pass| pass.crossed_at))),
        ),
        (
            "distance_m",
            Arc::new(Float64Array::from_iter_values(
                passes.iter().map(|pass| pass.distance_metres),
            )),
        ),
        (
            "samples_within",
            Arc::new(UInt32Array::from_iter_values(
                passes.iter().map(|pass| pass.samples_within),
            )),
        ),
    ])
}

fn instant_array(instants: impl Iterator<Item = DateTime<Utc>>) -> TimestampMillisecondArray {
    TimestampMillisecondArray::from_iter_values(instants.map(|t| t.timestamp_millis()))
        .with_timezone("UTC")
}

fn batch(columns: Vec<(&str, ArrayRef)>) -> PyResult<RecordBatch> {
    let schema = Schema::new(
        columns
            .iter()
            .map(|(name, array)| Field::new(*name, array.data_type().clone(), false))
            .collect::<Vec<_>>(),
    );
    RecordBatch::try_new(
        Arc::new(schema),
        columns.into_iter().map(|(_, array)| array).collect(),
    )
    .map_err(|err| PyRuntimeError::new_err(err.to_string()))
}

fn table(batch: RecordBatch) -> PyResult<PyTable> {
    let schema = batch.schema();
    PyTable::try_new(vec![batch], schema).map_err(|err| PyRuntimeError::new_err(err.to_string()))
}

fn unreadable(name: &str, reason: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(format!("the `{name}` column cannot be read: {reason}"))
}

fn value_error(err: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(err.to_string())
}

#[pymodule]
fn lookout_session_crossings(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(steps, module)?)?;
    Ok(())
}
