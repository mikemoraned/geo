//! The silver datasets a table built outside Rust can be written to.
//!
//! A caller that cannot hold a Rust row type names a dataset instead, and this is where the
//! name is resolved to its definition — so the columns such a table is checked against and
//! the layout it is written in come from the same place a Rust writer's do.
//!
//! Every silver dataset is here, not only the ones written from outside today. Which
//! derivation owns which dataset is a matter of which one runs, exactly as it is in Rust;
//! what the store enforces is the layer, and silver is the layer a derivation may replace.

use medallion::{Geometry, RowError, SilverTarget};

use crate::{
    SessionRow, SessionSampleRow, TrainSegmentRow, SESSION, SESSION_SAMPLE, TRAIN_SEGMENT,
};

/// A failure naming a dataset to write to.
#[derive(Debug, thiserror::Error)]
pub enum TargetError {
    #[error("no silver dataset named `{name}`; known: {known}", known = known())]
    NoSuchDataset { name: String },
    #[error(transparent)]
    Row(#[from] RowError),
}

/// How one dataset's definition is built, kept as a function so the row type stays a type.
type Definition = fn() -> Result<SilverTarget, RowError>;

/// Every silver dataset, as somewhere a table can be written.
///
/// The name comes from the dataset's own definition rather than being spelled again here, so
/// renaming a dataset moves its entry with it.
const TARGETS: [(&str, Definition); 3] = [
    (SESSION.name, || {
        SilverTarget::of::<SessionRow>(Geometry::LatLonAndProjected)
    }),
    (SESSION_SAMPLE.name, || {
        SilverTarget::of::<SessionSampleRow>(Geometry::LatLonAndProjected)
    }),
    (TRAIN_SEGMENT.name, || {
        SilverTarget::of::<TrainSegmentRow>(Geometry::LatLonAndProjected)
    }),
];

/// The dataset called `name`, as somewhere a table can be written.
pub fn silver_target(name: &str) -> Result<SilverTarget, TargetError> {
    let (_, definition) = TARGETS
        .iter()
        .find(|(dataset, _)| *dataset == name)
        .ok_or_else(|| TargetError::NoSuchDataset {
            name: name.to_string(),
        })?;
    Ok(definition()?)
}

/// The datasets that can be named, for an error that lists the choices.
fn known() -> String {
    TARGETS
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use medallion::Layer;

    use super::*;
    use crate::ALL;

    /// Every silver dataset is writable by name, so defining one and forgetting this is a
    /// failure here rather than an error the first time a notebook names it.
    #[test]
    fn every_silver_dataset_can_be_named() {
        for dataset in ALL.iter().filter(|d| d.layer == Layer::Silver) {
            assert!(
                silver_target(dataset.name).is_ok(),
                "{} cannot be named",
                dataset.name
            );
        }
    }

    /// And nothing outside silver is: the layer decides what may be replaced, and a table
    /// write replaces.
    #[test]
    fn no_dataset_outside_silver_can_be_named() {
        for dataset in ALL.iter().filter(|d| d.layer != Layer::Silver) {
            assert!(
                silver_target(dataset.name).is_err(),
                "{} can be named",
                dataset.name
            );
        }
    }

    #[test]
    fn an_unknown_name_is_refused_with_the_known_ones() {
        let err = silver_target("crossings").unwrap_err();

        assert!(err.to_string().contains("crossings"), "{err}");
        assert!(err.to_string().contains(SESSION.name), "{err}");
    }
}
