use std::marker::PhantomData;

use crate::layer::{Layer, LayerKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DatasetSpec<L> {
    pub name: &'static str,
    pub partition_key: Option<&'static str>,
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
        Self {
            name,
            partition_key: Some(partition_key),
            layer: PhantomData,
        }
    }

    pub const fn unpartitioned(name: &'static str) -> Self {
        Self {
            name,
            partition_key: None,
            layer: PhantomData,
        }
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
