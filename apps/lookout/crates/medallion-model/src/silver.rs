use medallion::{RowError, SilverTarget};

use crate::{
    SESSION, SESSION_CROSSING, SESSION_SAMPLE, SessionCrossingRow, SessionRow, SessionSampleRow,
    TRAIN_SEGMENT, TrainSegmentRow, WATER_CROSSING, WaterCrossingRow,
};

#[derive(Debug, thiserror::Error)]
pub enum TargetError {
    #[error("no silver dataset named `{name}`; known: {known}", known = known())]
    NoSuchDataset { name: String },
    #[error(transparent)]
    Row(#[from] RowError),
}

type Definition = fn() -> Result<SilverTarget, RowError>;

const TARGETS: [(&str, Definition); 5] = [
    (SESSION.name, SilverTarget::of::<SessionRow>),
    (SESSION_SAMPLE.name, SilverTarget::of::<SessionSampleRow>),
    (TRAIN_SEGMENT.name, SilverTarget::of::<TrainSegmentRow>),
    (WATER_CROSSING.name, SilverTarget::of::<WaterCrossingRow>),
    (
        SESSION_CROSSING.name,
        SilverTarget::of::<SessionCrossingRow>,
    ),
];

pub fn silver_target(name: &str) -> Result<SilverTarget, TargetError> {
    let (_, definition) = TARGETS
        .iter()
        .find(|(dataset, _)| *dataset == name)
        .ok_or_else(|| TargetError::NoSuchDataset {
            name: name.to_string(),
        })?;
    Ok(definition()?)
}

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
