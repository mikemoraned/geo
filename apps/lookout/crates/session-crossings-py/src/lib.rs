use arrow::array::{Array, ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow::compute::{cast, concat_batches};
use arrow::datatypes::{DataType, TimeUnit};
use chrono::{DateTime, Utc};
use domain::{CrossingId, Radius, SessionId};
use geo_types::{Geometry, Point};
use py_common::{runtime_error, table_of_batch, value_error};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pyo3_arrow::PyTable;
use serde::{Deserialize, Serialize};
use session_crossings::matching::{self, Crossing, DEFAULT_RADIUS, Sample, Visit};

/// The steps the session crossings matching takes for one session, as two tables.
///
/// `samples` holds the session's samples, with `t` and `geometry_projected`, as silver
/// `session_sample_v3` stores them. `crossings` holds the crossings to match against, with
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

    let visits_by_crossing = crossings
        .iter()
        .map(|crossing| {
            matching::visits(&session_id, &samples, crossing, radius).map_err(runtime_error)
        })
        .collect::<PyResult<Vec<Vec<Visit>>>>()?;

    let answer = PyDict::new(py);
    answer.set_item("near", table_of_batch(near_batch(&visits_by_crossing)?)?)?;
    answer.set_item(
        "passes",
        table_of_batch(passes_batch(&visits_by_crossing)?)?,
    )?;
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
    samples.sort_by(Sample::session_order);
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
        .ok_or_else(|| value_error(format!("the table has no `{name}` column")))
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

impl NearRow {
    fn example() -> Self {
        Self {
            crossing_id: "example".to_string(),
            t: DateTime::UNIX_EPOCH,
            distance_m: 0.0,
            visit: 0,
        }
    }
}

impl PassRow {
    fn example() -> Self {
        Self {
            crossing_id: "example".to_string(),
            crossed_at: DateTime::UNIX_EPOCH,
            distance_m: 0.0,
            samples_within: 1,
        }
    }
}

fn near_batch(visits_by_crossing: &[Vec<Visit>]) -> PyResult<RecordBatch> {
    let rows: Vec<NearRow> = visits_by_crossing
        .iter()
        .flat_map(|visits| {
            (0..).zip(visits).flat_map(|(number, visit)| {
                visit.near.iter().map(move |near| NearRow {
                    crossing_id: visit.pass.crossing_id.to_string(),
                    t: near.sample.t,
                    distance_m: near.distance_metres,
                    visit: number,
                })
            })
        })
        .collect();
    batch(&rows, &NearRow::example(), "t")
}

fn passes_batch(visits_by_crossing: &[Vec<Visit>]) -> PyResult<RecordBatch> {
    let rows: Vec<PassRow> = visits_by_crossing
        .iter()
        .flatten()
        .map(|Visit { pass, .. }| PassRow {
            crossing_id: pass.crossing_id.to_string(),
            crossed_at: pass.crossed_at,
            distance_m: pass.distance_metres,
            samples_within: pass.samples_within,
        })
        .collect();
    batch(&rows, &PassRow::example(), "crossed_at")
}

fn batch<T: Serialize>(rows: &[T], example: &T, instant: &str) -> PyResult<RecordBatch> {
    let fields = medallion::fields_of(example, &[instant]).map_err(runtime_error)?;
    serde_arrow::to_record_batch(&fields, &rows).map_err(runtime_error)
}

fn unreadable(name: &str, reason: impl std::fmt::Display) -> PyErr {
    value_error(format!("the `{name}` column cannot be read: {reason}"))
}

#[pymodule]
fn lookout_session_crossings(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(steps, module)?)?;
    Ok(())
}
