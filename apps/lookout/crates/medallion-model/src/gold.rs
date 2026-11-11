use medallion::{GoldTarget, RowError};

use crate::{
    EVALUATION_OUTCOME_V1, EVALUATION_RUN_V4, EVALUATION_SESSION_V1, EVALUATION_SKEW_V1,
    EVALUATION_SUMMARY_V1, EVALUATION_TRANSITION_V1, EvaluationOutcomeRowV1, EvaluationRunRowV4,
    EvaluationSessionRowV1, EvaluationSkewRowV1, EvaluationSummaryRowV1, EvaluationTransitionRowV1,
};

#[derive(Debug, thiserror::Error)]
pub enum GoldTargetError {
    #[error("no gold dataset named `{name}`; known: {known}", known = known())]
    NoSuchDataset { name: String },
    #[error(transparent)]
    Row(#[from] RowError),
}

type Definition = fn() -> Result<GoldTarget, RowError>;

const TARGETS: [(&str, Definition); 6] = [
    (EVALUATION_RUN_V4.name, GoldTarget::of::<EvaluationRunRowV4>),
    (
        EVALUATION_SESSION_V1.name,
        GoldTarget::of::<EvaluationSessionRowV1>,
    ),
    (
        EVALUATION_TRANSITION_V1.name,
        GoldTarget::of::<EvaluationTransitionRowV1>,
    ),
    (
        EVALUATION_OUTCOME_V1.name,
        GoldTarget::of::<EvaluationOutcomeRowV1>,
    ),
    (
        EVALUATION_SUMMARY_V1.name,
        GoldTarget::of::<EvaluationSummaryRowV1>,
    ),
    (
        EVALUATION_SKEW_V1.name,
        GoldTarget::of::<EvaluationSkewRowV1>,
    ),
];

pub fn gold_target(name: &str) -> Result<GoldTarget, GoldTargetError> {
    let (_, definition) = TARGETS
        .iter()
        .find(|(dataset, _)| *dataset == name)
        .ok_or_else(|| GoldTargetError::NoSuchDataset {
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
    fn every_gold_dataset_can_be_named() {
        for dataset in ALL.iter().filter(|dataset| dataset.layer == Layer::Gold) {
            assert_eq!(
                gold_target(dataset.name)
                    .map(|target| target.spec().name)
                    .ok(),
                Some(dataset.name)
            );
        }
    }

    #[test]
    fn a_name_no_gold_dataset_has_is_refused() {
        assert!(matches!(
            gold_target("train_session_v3"),
            Err(GoldTargetError::NoSuchDataset { .. })
        ));
    }
}
