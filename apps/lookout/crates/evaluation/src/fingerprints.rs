use std::collections::{BTreeMap, BTreeSet};

use futures::future::try_join_all;
use medallion::{DatasetSpec, FingerprintError, Root, layers};
use medallion_model::{
    TRAIN_SESSION_CROSSING_V1, TRAIN_SESSION_SAMPLE_V3, TRAIN_SESSION_V3, WATER_CROSSING_V2,
};

const FINGERPRINTED: [DatasetSpec<layers::Silver>; 4] = [
    TRAIN_SESSION_V3,
    TRAIN_SESSION_SAMPLE_V3,
    TRAIN_SESSION_CROSSING_V1,
    WATER_CROSSING_V2,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprints(BTreeMap<String, String>);

impl Fingerprints {
    pub async fn read(root: &Root) -> Result<Self, FingerprintError> {
        let read = try_join_all(FINGERPRINTED.iter().map(|dataset| async move {
            let fingerprint = root.dataset(*dataset).fingerprint().await?;
            Ok::<_, FingerprintError>((dataset.name.to_string(), fingerprint.to_string()))
        }))
        .await?;
        Ok(Self(read.into_iter().collect()))
    }

    pub fn changed_since(&self, earlier: &Self) -> Vec<String> {
        self.0
            .keys()
            .chain(earlier.0.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|dataset| self.0.get(*dataset) != earlier.0.get(*dataset))
            .cloned()
            .collect()
    }
}

impl From<BTreeMap<String, String>> for Fingerprints {
    fn from(by_dataset: BTreeMap<String, String>) -> Self {
        Self(by_dataset)
    }
}

impl From<Fingerprints> for BTreeMap<String, String> {
    fn from(fingerprints: Fingerprints) -> Self {
        fingerprints.0
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use medallion::Row;
    use medallion_model::TrainSessionCrossingRowV1;

    use super::*;

    fn fingerprints(pairs: &[(&str, &str)]) -> Fingerprints {
        Fingerprints::from(
            pairs
                .iter()
                .map(|&(dataset, fingerprint)| (dataset.to_string(), fingerprint.to_string()))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    #[test]
    fn only_the_datasets_whose_fingerprints_differ_have_changed() {
        let before = fingerprints(&[("a", "f1"), ("b", "f2")]);
        let after = fingerprints(&[("a", "f1"), ("b", "changed")]);

        assert_eq!(after.changed_since(&before), ["b"]);
        assert!(after.changed_since(&after).is_empty());
    }

    #[test]
    fn a_dataset_fingerprinted_on_one_side_alone_has_changed() {
        let before = fingerprints(&[("a", "f1"), ("gone", "f2")]);
        let after = fingerprints(&[("a", "f1"), ("new", "f3")]);

        assert_eq!(after.changed_since(&before), ["gone", "new"]);
    }

    #[tokio::test]
    async fn a_fingerprint_moves_with_the_dataset_that_changes_and_no_other() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = crate::fixtures::store(tmp.path(), 2).await;
        let before = Fingerprints::read(&root).await.expect("fingerprints");

        medallion::write_rows(
            &root,
            &[TrainSessionCrossingRowV1 {
                crossed_at: DateTime::<Utc>::UNIX_EPOCH,
                ..TrainSessionCrossingRowV1::example()
            }],
        )
        .await
        .expect("another pass");
        let after = Fingerprints::read(&root).await.expect("fingerprints");

        assert_eq!(
            after.changed_since(&before),
            [TRAIN_SESSION_CROSSING_V1.name]
        );
    }

    #[tokio::test]
    async fn a_dataset_never_written_has_no_fingerprint() {
        let tmp = tempfile::tempdir().expect("a temporary directory");

        let refused = Fingerprints::read(&Root::new(tmp.path())).await;

        assert!(
            matches!(refused, Err(FingerprintError::Unwritten { .. })),
            "{refused:?}"
        );
    }
}
