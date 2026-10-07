use medallion::{DatasetSpec, layers};

use crate::{
    ACCEL_READING, DEVICE_SESSION, EXTRACT_MANIFEST, GPS_READING, MOTIS_SEGMENT, MOTIS_SEGMENT_V2,
    MOTIS_SOURCE, OVERTURE_EXTRACT, RAW_SAMPLE,
};

#[derive(Debug, thiserror::Error)]
#[error("no bronze dataset named `{name}`; known: {known}", known = known())]
pub struct NoSuchBronzeDataset {
    pub name: String,
}

const DATASETS: [DatasetSpec<layers::Bronze>; 9] = [
    RAW_SAMPLE,
    GPS_READING,
    ACCEL_READING,
    DEVICE_SESSION,
    MOTIS_SEGMENT,
    MOTIS_SEGMENT_V2,
    MOTIS_SOURCE,
    OVERTURE_EXTRACT,
    EXTRACT_MANIFEST,
];

pub fn bronze_dataset(name: &str) -> Result<DatasetSpec<layers::Bronze>, NoSuchBronzeDataset> {
    DATASETS
        .iter()
        .copied()
        .find(|dataset| dataset.name == name)
        .ok_or_else(|| NoSuchBronzeDataset {
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
    fn every_bronze_dataset_can_be_named() {
        for dataset in ALL.iter().filter(|d| d.layer == Layer::Bronze) {
            assert!(
                bronze_dataset(dataset.name).is_ok(),
                "{} cannot be named",
                dataset.name
            );
        }
    }

    #[test]
    fn no_dataset_outside_bronze_can_be_named() {
        for dataset in ALL.iter().filter(|d| d.layer != Layer::Bronze) {
            assert!(
                bronze_dataset(dataset.name).is_err(),
                "{} can be named",
                dataset.name
            );
        }
    }

    #[test]
    fn an_unknown_name_is_refused_with_the_known_ones() {
        let err = bronze_dataset("segment").unwrap_err();

        assert!(err.to_string().contains("segment"), "{err}");
        assert!(err.to_string().contains(OVERTURE_EXTRACT.name), "{err}");
    }
}
