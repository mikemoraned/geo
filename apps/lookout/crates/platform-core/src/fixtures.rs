//! The platforms and fixes this crate's tests share.
//!
//! A platform here shows the state as it stands, so a test reads what the core holds rather
//! than how some shell formats it. The projections themselves are tested where they live,
//! against the screens they are for.

use chrono::{DateTime, Utc};
use domain::{CrossingCompact, Gps, Sample};

use crate::Float;
use crate::app::{Event, Model, Shell};
use crate::battery::Charge;
use crate::connected::{Connected, on_the_globe};
use crate::standalone::Standalone;

/// What the core knows, unformatted.
#[derive(Debug, PartialEq)]
pub struct State {
    pub now: Option<DateTime<Utc>>,
    pub latitude: Option<Float>,
    pub speed_mps: Option<Float>,
    pub charge: Option<Charge>,
    pub crossings: usize,
    pub predicted: usize,
}

fn state<S: Shell<Crossings = Vec<CrossingCompact<Float>>>>(model: &Model<S>) -> State {
    State {
        now: model.now(),
        latitude: model.fix().map(Sample::latitude),
        speed_mps: model.speed_mps(),
        charge: model.charge(),
        crossings: model.crossings().map_or(0, Vec::len),
        predicted: model.predictions().len(),
    }
}

/// A platform carrying an empty set, as a device carries a full one: the predictor exists
/// from the start and predicts nothing.
#[derive(Debug, Default, Clone, Copy)]
pub struct Bare;

impl Shell for Bare {
    type ViewModel = State;
    type Crossings = Vec<CrossingCompact<Float>>;

    fn project(model: &Model<Self>) -> State {
        state(model)
    }
}

impl Standalone for Bare {
    fn carried() -> Self::Crossings {
        Vec::new()
    }
}

/// A platform with no set of its own, as a browser is before it has fetched one.
#[derive(Debug, Default, Clone, Copy)]
pub struct Late;

impl Shell for Late {
    type ViewModel = State;
    type Crossings = Vec<CrossingCompact<Float>>;

    fn project(model: &Model<Self>) -> State {
        state(model)
    }
}

impl Connected for Late {
    fn received(points: Vec<CrossingCompact<f64>>) -> Self::Crossings {
        on_the_globe(points)
    }
}

pub fn instant() -> DateTime<Utc> {
    DateTime::from_timestamp(1_785_098_609, 0).expect("an instant")
}

/// Dresden Hauptbahnhof's longitude, which the fixes here stay on while the latitude moves
/// them north.
pub const DRESDEN_LON: f64 = 13.7322;

/// Dresden Hauptbahnhof, at a train's speed.
pub fn at_the_station() -> Gps<f64> {
    Gps::at(51.0403, DRESDEN_LON)
        .expect("on the globe")
        .with_accuracy_metres(Some(5.0))
        .with_speed_mps(Some(27.8))
}

/// A crossing as a shell sends one: degrees, latitude first.
pub fn crossing(id: u32, latitude: f64, longitude: f64) -> CrossingCompact<f64> {
    CrossingCompact::at(id, latitude, longitude).expect("on the globe")
}

pub fn reported(t: DateTime<Utc>, gps: Gps<f64>) -> Event {
    Event::Position(Sample::new(t, gps))
}
