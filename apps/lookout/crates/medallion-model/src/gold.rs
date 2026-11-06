use arrow::array::RecordBatch;
use medallion::{DatasetSpec, Row, RowError, layers};

use crate::{
    EVALUATION_OUTCOME_V1, EVALUATION_SUMMARY_V1, EVALUATION_TRANSITION_V1, EvaluationOutcomeRowV1,
    EvaluationSummaryRowV1, EvaluationTransitionRowV1,
};

#[derive(Debug, thiserror::Error)]
pub enum GoldTargetError {
    #[error("no gold dataset named `{name}`; known: {known}", known = known())]
    NoSuchDataset { name: String },
}

#[derive(Debug, Clone, Copy)]
pub struct GoldTarget {
    spec: DatasetSpec<layers::Gold>,
    rows_of: fn(&[RecordBatch]) -> Result<RecordBatch, RowError>,
}

impl GoldTarget {
    pub fn of<R: Row<Layer = layers::Gold>>() -> Self {
        Self {
            spec: R::DATASET,
            rows_of: rows_of::<R>,
        }
    }

    pub fn spec(&self) -> DatasetSpec<layers::Gold> {
        self.spec
    }

    pub fn rows(&self, table: &[RecordBatch]) -> Result<RecordBatch, RowError> {
        (self.rows_of)(table)
    }
}

fn rows_of<R: Row>(table: &[RecordBatch]) -> Result<RecordBatch, RowError> {
    let rows = table
        .iter()
        .map(medallion::from_batch::<R>)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<R>>();
    medallion::batch(&rows)
}

type Definition = fn() -> GoldTarget;

const TARGETS: [(&str, Definition); 3] = [
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
];

pub fn gold_target(name: &str) -> Result<GoldTarget, GoldTargetError> {
    TARGETS
        .iter()
        .find(|(dataset, _)| *dataset == name)
        .map(|(_, target)| target())
        .ok_or_else(|| GoldTargetError::NoSuchDataset {
            name: name.to_string(),
        })
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
    use std::sync::Arc;

    use arrow::array::ArrayRef;
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

    fn transitions() -> RecordBatch {
        medallion::batch(&[
            EvaluationTransitionRowV1::example(),
            EvaluationTransitionRowV1::example(),
        ])
        .expect("a batch of transitions")
    }

    #[test]
    fn a_tables_columns_are_put_in_the_row_types_order() {
        let table = transitions();
        let reversed: Vec<usize> = (0..table.num_columns()).rev().collect();
        let shuffled = table.project(&reversed).expect("the columns reversed");

        let rows = gold_target(EVALUATION_TRANSITION_V1.name)
            .expect("a target")
            .rows(&[shuffled])
            .expect("the rows");

        assert_eq!(rows, table);
    }

    #[test]
    fn a_table_lacking_a_column_of_the_row_type_is_refused() {
        let table = transitions();
        let without_to: Vec<(String, ArrayRef)> = table
            .schema()
            .fields()
            .iter()
            .zip(table.columns())
            .filter(|(field, _)| field.name() != "to")
            .map(|(field, column)| (field.name().clone(), Arc::clone(column)))
            .collect();
        let lacking = RecordBatch::try_from_iter(without_to).expect("a batch without `to`");

        let refused = gold_target(EVALUATION_TRANSITION_V1.name)
            .expect("a target")
            .rows(&[lacking]);

        assert!(refused.is_err());
    }
}
