use medallion::{RowError, SilverTarget};

use crate::{
    SESSION_CROSSING, SESSION_SAMPLE, SESSION_V2, SessionCrossingRow, SessionRowV2,
    SessionSampleRow, TRAIN_SEGMENT_V2, TRAIN_SESSION, TRAIN_SESSION_SAMPLE, TrainSegmentRowV2,
    TrainSessionRow, TrainSessionSampleRow, WATER_CROSSING_V2, WaterCrossingRowV2,
};

#[derive(Debug, thiserror::Error)]
pub enum TargetError {
    #[error("no silver dataset named `{name}`; known: {known}", known = known())]
    NoSuchDataset { name: String },
    #[error(transparent)]
    Row(#[from] RowError),
}

type Definition = fn() -> Result<SilverTarget, RowError>;

const TARGETS: [(&str, Definition); 7] = [
    (SESSION_V2.name, SilverTarget::of::<SessionRowV2>),
    (SESSION_SAMPLE.name, SilverTarget::of::<SessionSampleRow>),
    (TRAIN_SEGMENT_V2.name, SilverTarget::of::<TrainSegmentRowV2>),
    (TRAIN_SESSION.name, SilverTarget::of::<TrainSessionRow>),
    (
        TRAIN_SESSION_SAMPLE.name,
        SilverTarget::of::<TrainSessionSampleRow>,
    ),
    (
        WATER_CROSSING_V2.name,
        SilverTarget::of::<WaterCrossingRowV2>,
    ),
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
    fn the_train_sessions_can_be_named() {
        for name in [TRAIN_SESSION.name, TRAIN_SESSION_SAMPLE.name] {
            assert!(silver_target(name).is_ok(), "{name} cannot be named");
        }
    }

    #[test]
    fn an_unknown_name_is_refused_with_the_known_ones() {
        let err = silver_target("crossings").unwrap_err();

        assert!(err.to_string().contains("crossings"), "{err}");
        assert!(err.to_string().contains(SESSION_V2.name), "{err}");
    }
}
