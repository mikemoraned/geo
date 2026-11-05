use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use num_traits::float::TotalOrder;
use serde::{Deserialize, Serialize};

use domain::{CrossingCompactId, Precision, Sample};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Event<P: Precision> {
    Sampled(Sample<P>),
    Elapsed(DateTime<Utc>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Confidence {
    Low,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Prediction<P: Precision> {
    pub metres: P,
    pub at: Option<DateTime<Utc>>,
    pub confidence: Confidence,
}

pub type Predictions<P> = BTreeMap<CrossingCompactId, Prediction<P>>;

pub fn nearest_first<P: Precision + TotalOrder>(
    predictions: &Predictions<P>,
) -> Vec<(CrossingCompactId, &Prediction<P>)> {
    let mut nearest: Vec<(CrossingCompactId, &Prediction<P>)> = predictions
        .iter()
        .map(|(&crossing_compact_id, prediction)| (crossing_compact_id, prediction))
        .collect();
    nearest.sort_by(|(_, one), (_, other)| one.metres.total_cmp(&other.metres));
    nearest
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ObserveError {
    #[error("an event at {at} is behind the clock at {now}")]
    OutOfOrder {
        now: DateTime<Utc>,
        at: DateTime<Utc>,
    },
}

pub trait Predict<P: Precision> {
    fn observe(&mut self, event: Event<P>) -> Result<(), ObserveError>;

    fn predictions(&self) -> &Predictions<P>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(metres: f64) -> Prediction<f64> {
        Prediction {
            metres,
            at: None,
            confidence: Confidence::Low,
        }
    }

    #[test]
    fn nearest_first_orders_by_distance_rather_than_by_crossing() {
        let predictions: Predictions<f64> = [(1, 300.0), (2, 100.0), (3, 200.0)]
            .into_iter()
            .map(|(id, metres)| (CrossingCompactId::from(id), at(metres)))
            .collect();

        let ids: Vec<u32> = nearest_first(&predictions)
            .into_iter()
            .map(|(crossing_compact_id, _)| crossing_compact_id.get())
            .collect();

        assert_eq!(ids, vec![2, 3, 1]);
    }
}
