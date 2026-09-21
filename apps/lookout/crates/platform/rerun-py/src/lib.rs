//! The crow-flies predictor, as a python object.
//!
//! The runner replaying a session lives in python, because the rerun SDK carries more of the
//! blueprint API there than in Rust. It binds the predictor rather than reimplementing it, so
//! what it draws is what every other shell answers.
//!
//! Feed a session through it in `t` order:
//!
//! ```python
//! from lookout_predictor import CrowFlies
//!
//! predictor = CrowFlies([(crossing_id, lat, lon), ...], radius_metres=5000.0)
//! for row in samples:
//!     predictor.observe_sample(row.t, row.lat, row.lon, speed_mps=row.speed)
//!     for prediction in predictor.predictions():
//!         ...
//! ```
//!
//! Nothing is serialised across the boundary: python holds the state machine itself, and a
//! call into it runs the predictor's own code.
//!
//! It measures in `f64`, which is what the store holds and what a python float is. Instants
//! are aware datetimes, in whatever timezone the caller has them in.

use chrono::{DateTime, FixedOffset, Utc};
use domain::{CrossingCompact, Sample};
use predictor::{
    CrowFlies as CrowFliesPredictor, DEFAULT_RADIUS_METRES, Event, ObserveError, Predict,
};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// One crossing the predictor expects us to reach.
#[pyclass(frozen, get_all, eq, skip_from_py_object)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prediction {
    crossing_compact_id: u32,
    /// The straight-line distance from the latest fix, in metres.
    metres: f64,
    /// When we reach it at the speed of the latest fix, absent where there is no speed to
    /// divide by. An instant rather than a countdown, so it stays true while the clock
    /// advances between fixes.
    at: Option<DateTime<Utc>>,
}

#[pymethods]
impl Prediction {
    fn __repr__(&self) -> String {
        format!(
            "Prediction(crossing_compact_id={}, metres={:.1}, at={})",
            self.crossing_compact_id,
            self.metres,
            match self.at {
                Some(at) => at.to_rfc3339(),
                None => "None".to_string(),
            }
        )
    }
}

/// A predictor that measures in straight lines: the distance to each crossing within the
/// radius, and when we reach it at the speed we are going.
///
/// The track's own geometry plays no part, so a bend or a river meander puts a crossing
/// nearer, and sooner, than the rails can reach it.
#[pyclass]
pub struct CrowFlies {
    inner: CrowFliesPredictor<f64>,
}

#[pymethods]
impl CrowFlies {
    /// A predictor over `crossings`, each `(id, latitude_degrees, longitude_degrees)`,
    /// reporting everything within `radius_metres`.
    #[new]
    #[pyo3(signature = (crossings, *, radius_metres=DEFAULT_RADIUS_METRES))]
    fn new(crossings: Vec<(u32, f64, f64)>, radius_metres: f64) -> PyResult<Self> {
        let crossings = crossings
            .into_iter()
            .map(|(id, latitude, longitude)| {
                CrossingCompact::at(id, latitude, longitude)
                    .map_err(|err| PyValueError::new_err(err.to_string()))
            })
            .collect::<PyResult<Vec<_>>>()?;

        Ok(Self {
            inner: CrowFliesPredictor::new(crossings, radius_metres),
        })
    }

    /// Observes one fix, which moves the position and advances the clock to its own instant.
    ///
    /// `t` is an aware datetime in any timezone, since a store hands one back in whichever
    /// its engine holds. A naive one is refused: it names no instant.
    ///
    /// Everything past the position is what the source happened to know. A field left out
    /// stays unknown rather than being invented: with no speed reported, the step from the
    /// previous fix says how fast we are going, and with no previous fix there is no time to
    /// give.
    #[pyo3(signature = (
        t,
        latitude,
        longitude,
        *,
        altitude_metres=None,
        speed_mps=None,
        heading_degrees=None,
        accuracy_metres=None,
        satellites=None,
        hdop=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn observe_sample(
        &mut self,
        t: DateTime<FixedOffset>,
        latitude: f64,
        longitude: f64,
        altitude_metres: Option<f64>,
        speed_mps: Option<f64>,
        heading_degrees: Option<f64>,
        accuracy_metres: Option<f64>,
        satellites: Option<u32>,
        hdop: Option<f64>,
    ) -> PyResult<()> {
        let sample = Sample::at(t.to_utc(), latitude, longitude)
            .map_err(|err| PyValueError::new_err(err.to_string()))?
            .with_altitude_metres(altitude_metres)
            .with_speed_mps(speed_mps)
            .with_heading_degrees(heading_degrees)
            .with_accuracy_metres(accuracy_metres)
            .with_satellites(satellites)
            .with_hdop(hdop);

        self.observe(Event::Sampled(sample))
    }

    /// The crossings it predicts we reach, nearest first.
    fn predictions(&self) -> Vec<Prediction> {
        self.inner
            .predictions()
            .iter()
            .map(|prediction| Prediction {
                crossing_compact_id: prediction.crossing_compact_id.get(),
                metres: prediction.metres,
                at: prediction.at,
            })
            .collect()
    }
}

impl CrowFlies {
    /// An event out of order is the caller's mistake — a session replays in `t` order — so it
    /// raises rather than passing in silence, and changes nothing.
    fn observe(&mut self, event: Event<f64>) -> PyResult<()> {
        self.inner
            .observe(event)
            .map_err(|err: ObserveError| PyValueError::new_err(err.to_string()))
    }
}

#[pymodule]
fn lookout_predictor(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<CrowFlies>()?;
    module.add_class::<Prediction>()?;
    module.add("DEFAULT_RADIUS_METRES", DEFAULT_RADIUS_METRES)?;
    Ok(())
}
