use medallion::{DatasetSpec, layers};

use crate::MOTIS_POLL_V2;

#[derive(Debug, thiserror::Error)]
#[error("no landing dataset named `{name}`; known: {known}", known = known())]
pub struct NoSuchLandingDataset {
    pub name: String,
}

const DATASETS: [DatasetSpec<layers::Landing>; 1] = [MOTIS_POLL_V2];

pub fn landing_dataset(name: &str) -> Result<DatasetSpec<layers::Landing>, NoSuchLandingDataset> {
    DATASETS
        .iter()
        .copied()
        .find(|dataset| dataset.name == name)
        .ok_or_else(|| NoSuchLandingDataset {
            name: name.to_string(),
        })
}

fn known() -> String {
    DATASETS
        .iter()
        .map(|dataset| dataset.name)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use medallion::Layer;

    use super::*;
    use crate::ALL;

    #[test]
    fn every_landing_dataset_can_be_named() {
        for dataset in ALL.iter().filter(|d| d.layer == Layer::Landing) {
            assert!(
                landing_dataset(dataset.name).is_ok(),
                "{} cannot be named",
                dataset.name
            );
        }
    }

    #[test]
    fn no_dataset_outside_landing_can_be_named() {
        for dataset in ALL.iter().filter(|d| d.layer != Layer::Landing) {
            assert!(
                landing_dataset(dataset.name).is_err(),
                "{} can be named",
                dataset.name
            );
        }
    }

    #[test]
    fn an_unknown_name_is_refused_with_the_known_ones() {
        let err = landing_dataset("poll").unwrap_err();

        assert!(err.to_string().contains("poll"), "{err}");
        assert!(err.to_string().contains("motis_poll_v2"), "{err}");
    }
}
