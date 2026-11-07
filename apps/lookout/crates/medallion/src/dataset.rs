use std::marker::PhantomData;

use crate::layer::{Layer, LayerKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DatasetSpec<L> {
    pub name: &'static str,
    pub partition_key: Option<&'static str>,
    deeper_keys: &'static [&'static str],
    layer: PhantomData<L>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DatasetInfo {
    pub layer: Layer,
    pub name: &'static str,
    pub partition_key: Option<&'static str>,
}

impl<L> DatasetSpec<L> {
    pub const fn partitioned(name: &'static str, partition_key: &'static str) -> Self {
        Self::partitioned_within(name, partition_key, &[])
    }

    pub const fn partitioned_within(
        name: &'static str,
        partition_key: &'static str,
        deeper_keys: &'static [&'static str],
    ) -> Self {
        Self {
            name,
            partition_key: Some(partition_key),
            deeper_keys,
            layer: PhantomData,
        }
    }

    pub const fn unpartitioned(name: &'static str) -> Self {
        Self {
            name,
            partition_key: None,
            deeper_keys: &[],
            layer: PhantomData,
        }
    }

    pub fn partition_levels(&self) -> Vec<&'static str> {
        self.partition_key
            .into_iter()
            .chain(self.deeper_keys.iter().copied())
            .collect()
    }
}

impl<L: LayerKind> DatasetSpec<L> {
    pub const fn layer(&self) -> Layer {
        L::LAYER
    }

    pub const fn info(&self) -> DatasetInfo {
        DatasetInfo {
            layer: L::LAYER,
            name: self.name,
            partition_key: self.partition_key,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::layers;

    #[test]
    fn a_dataset_partitioned_within_its_key_has_the_deeper_levels_beneath_it() {
        let nested: DatasetSpec<layers::Gold> =
            DatasetSpec::partitioned_within("nested", "made_date", &["batch", "part"]);

        assert_eq!(nested.partition_levels(), ["made_date", "batch", "part"]);
    }

    #[test]
    fn a_partitioned_dataset_has_its_key_as_its_one_level() {
        let dated: DatasetSpec<layers::Silver> = DatasetSpec::partitioned("dated", "made_date");

        assert_eq!(dated.partition_levels(), ["made_date"]);
    }

    #[test]
    fn an_unpartitioned_dataset_has_no_levels() {
        let whole: DatasetSpec<layers::Silver> = DatasetSpec::unpartitioned("whole");

        assert!(whole.partition_levels().is_empty());
    }
}
