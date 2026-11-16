use medallion::{GoldTarget, RowError};

use crate::{
    EVALUATION_OUTCOME_V2, EVALUATION_RUN_V6, EVALUATION_SESSION_V2, EVALUATION_SKEW_V2,
    EVALUATION_SPLIT_V1, EVALUATION_SUMMARY_V2, EVALUATION_SUMMARY_V3, EVALUATION_TRANSITION_V1,
    EvaluationOutcomeRowV2, EvaluationRunRowV6, EvaluationSessionRowV2, EvaluationSkewRowV2,
    EvaluationSplitRowV1, EvaluationSummaryRowV2, EvaluationSummaryRowV3,
    EvaluationTransitionRowV1,
};

#[derive(Debug, thiserror::Error)]
pub enum GoldTargetError {
    #[error("no gold dataset named `{name}`; known: {known}", known = known())]
    NoSuchDataset { name: String },
    #[error(transparent)]
    Row(#[from] RowError),
}

type Definition = fn() -> Result<GoldTarget, RowError>;

const TARGETS: [(&str, Definition); 8] = [
    (EVALUATION_RUN_V6.name, GoldTarget::of::<EvaluationRunRowV6>),
    (
        EVALUATION_SESSION_V2.name,
        GoldTarget::of::<EvaluationSessionRowV2>,
    ),
    (
        EVALUATION_TRANSITION_V1.name,
        GoldTarget::of::<EvaluationTransitionRowV1>,
    ),
    (
        EVALUATION_OUTCOME_V2.name,
        GoldTarget::of::<EvaluationOutcomeRowV2>,
    ),
    (
        EVALUATION_SUMMARY_V2.name,
        GoldTarget::of::<EvaluationSummaryRowV2>,
    ),
    (
        EVALUATION_SUMMARY_V3.name,
        GoldTarget::of::<EvaluationSummaryRowV3>,
    ),
    (
        EVALUATION_SKEW_V2.name,
        GoldTarget::of::<EvaluationSkewRowV2>,
    ),
    (
        EVALUATION_SPLIT_V1.name,
        GoldTarget::of::<EvaluationSplitRowV1>,
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
