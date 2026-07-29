//! The layers data flows through, and the directory each occupies under the root.

/// A layer of the store. See `docs/medallion.md` for what belongs in each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Landing,
    Bronze,
    Silver,
    Gold,
}

/// An attempt to replace or delete data in a layer that only ever grows.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{dataset} is in {layer}, which is append-only: its data cannot be replaced or deleted")]
pub struct AppendOnly {
    pub layer: &'static str,
    pub dataset: String,
}

impl Layer {
    /// Whether a dataset in this layer may have its data replaced or deleted.
    ///
    /// Landing and bronze are the record of what was observed. Nothing derives them, so
    /// nothing can put them back, and they only ever grow. Silver and gold are derived
    /// wholesale from them, so replacing a partition of one costs a rerun at most — which is
    /// what lets a rebuild replace what it no longer produces.
    pub fn permits_replacement(self) -> bool {
        matches!(self, Layer::Silver | Layer::Gold)
    }

    /// The directory name this layer occupies under the root.
    pub fn as_str(self) -> &'static str {
        match self {
            Layer::Landing => "landing",
            Layer::Bronze => "bronze",
            Layer::Silver => "silver",
            Layer::Gold => "gold",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The layers holding what was observed refuse replacement; the derived ones permit it.
    #[test]
    fn only_the_derived_layers_permit_replacement() {
        assert!(!Layer::Landing.permits_replacement());
        assert!(!Layer::Bronze.permits_replacement());
        assert!(Layer::Silver.permits_replacement());
        assert!(Layer::Gold.permits_replacement());
    }

    #[test]
    fn directory_names_are_the_layer_names() {
        assert_eq!(Layer::Landing.as_str(), "landing");
        assert_eq!(Layer::Bronze.as_str(), "bronze");
        assert_eq!(Layer::Silver.as_str(), "silver");
        assert_eq!(Layer::Gold.as_str(), "gold");
    }
}
