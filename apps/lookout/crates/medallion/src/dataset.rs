//! What a dataset is: where it lives and how it is partitioned.
//!
//! A dataset is named once as a [`DatasetSpec`] and passed around as that value, so its
//! layer and partition key travel with it. Readers and writers then agree on the layout by
//! construction rather than by each repeating a string.
//!
//! The layer is the spec's **type**, not a field: `DatasetSpec<layers::Bronze>` and
//! `DatasetSpec<layers::Silver>` are different types, which is what lets the store offer
//! rewriting and deleting only for the layers that permit them.
//!
//! The store defines the shape; the datasets themselves are defined by whoever owns the
//! data, so this crate holds no list of them.

use std::marker::PhantomData;

use crate::layer::{Layer, LayerKind};

/// Where a dataset lives and how it is partitioned, with its layer as a type parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DatasetSpec<L> {
    /// Its directory name under its layer.
    pub name: &'static str,
    /// The Hive key its partitions are keyed on, or `None` for a dataset small enough
    /// that it is always read whole and so is left unpartitioned.
    pub partition_key: Option<&'static str>,
    layer: PhantomData<L>,
}

/// One dataset's definition with its layer as a value rather than a type, for the checks
/// that have to cover datasets of different layers together — an array of specs cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DatasetInfo {
    pub layer: Layer,
    pub name: &'static str,
    pub partition_key: Option<&'static str>,
}

impl<L> DatasetSpec<L> {
    /// A dataset whose files sit under a `key=value` directory per partition.
    pub const fn partitioned(name: &'static str, partition_key: &'static str) -> Self {
        Self {
            name,
            partition_key: Some(partition_key),
            layer: PhantomData,
        }
    }

    /// A dataset whose files sit directly under its own directory.
    pub const fn unpartitioned(name: &'static str) -> Self {
        Self {
            name,
            partition_key: None,
            layer: PhantomData,
        }
    }
}

impl<L: LayerKind> DatasetSpec<L> {
    /// The layer it belongs to.
    pub const fn layer(&self) -> Layer {
        L::LAYER
    }

    /// The same definition with its layer as a value, for a caller holding datasets of
    /// several layers at once.
    pub const fn info(&self) -> DatasetInfo {
        DatasetInfo {
            layer: L::LAYER,
            name: self.name,
            partition_key: self.partition_key,
        }
    }
}
