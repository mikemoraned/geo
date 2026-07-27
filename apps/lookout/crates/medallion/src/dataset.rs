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
    /// The Hive key its partitions are keyed on, or `None` for a dataset small enough
    /// that it is always read whole and so is left unpartitioned.
    pub partition_key: Option<&'static str>,
}

impl DatasetSpec {
    /// A dataset whose files sit under a `key=value` directory per partition.
    pub const fn partitioned(
        layer: Layer,
        name: &'static str,
        partition_key: &'static str,
    ) -> Self {
        Self {
            layer,
            name,
            partition_key: Some(partition_key),
        }
    }

    /// A dataset whose files sit directly under its own directory.
    pub const fn unpartitioned(layer: Layer, name: &'static str) -> Self {
        Self {
            layer,
            name,
            partition_key: None,
        }
    }
}
