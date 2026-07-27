//! What a dataset is: where it lives and how it is partitioned.
//!
//! A dataset is named once as a [`DatasetSpec`] and passed around as that value, so its
//! layer and partition key travel with it. Readers and writers then agree on the layout by
//! construction rather than by each repeating a string.
//!
//! The store defines the shape; the datasets themselves are defined by whoever owns the
//! data, so this crate holds no list of them.

use crate::layer::Layer;

/// Where a dataset lives and how it is partitioned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DatasetSpec {
    /// The layer it belongs to.
    pub layer: Layer,
    /// Its directory name under that layer.
    pub name: &'static str,
    /// The Hive key its partitions are keyed on.
    pub partition_key: &'static str,
}

impl DatasetSpec {
    pub const fn new(layer: Layer, name: &'static str, partition_key: &'static str) -> Self {
        Self {
            layer,
            name,
            partition_key,
        }
    }
}
