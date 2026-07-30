//! The layers data flows through, and the directory each occupies under the root.

/// A layer of the store. See `docs/medallion.md` for what belongs in each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Landing,
    Bronze,
    Silver,
    Gold,
}

/// The layers as types, so a dataset carries its layer in its own type and the operations a
/// layer does not permit are not there to call.
///
/// A marker names the same layer as the [`Layer`] variant it shares its name with; the
/// variant is what a path or an error message is built from, the marker is what a dataset
/// definition is written in terms of.
pub mod layers {
    use super::{Layer, LayerKind, Replaceable};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Landing;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Bronze;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Silver;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Gold;

    impl LayerKind for Landing {
        const LAYER: Layer = Layer::Landing;
    }

    impl LayerKind for Bronze {
        const LAYER: Layer = Layer::Bronze;
    }

    impl LayerKind for Silver {
        const LAYER: Layer = Layer::Silver;
    }

    impl LayerKind for Gold {
        const LAYER: Layer = Layer::Gold;
    }

    impl Replaceable for Silver {}

    impl Replaceable for Gold {}
}

/// A layer, as a type. Implemented by each marker in [`layers`].
///
/// The supertraits are what the markers are: plain unit types carrying no data, so a dataset
/// parameterised by one is as copyable and comparable as the definition it stands for.
pub trait LayerKind: Copy + std::fmt::Debug + PartialEq + Eq + std::hash::Hash {
    /// The layer this type stands for.
    const LAYER: Layer;
}

/// A layer whose data may be replaced or deleted: silver and gold, and nothing else.
///
/// The operations that rewrite or remove a partition are implemented only for datasets in
/// these layers, so replacing what cannot be re-derived is a missing method rather than an
/// error to handle — see [`Layer::permits_replacement`] for why the line falls here.
pub trait Replaceable: LayerKind {}

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

    /// The markers stand for the same layers the variants do, and only the derived ones are
    /// replaceable — the same split, expressed in types.
    #[test]
    fn the_markers_agree_with_the_variants() {
        assert_eq!(layers::Landing::LAYER, Layer::Landing);
        assert_eq!(layers::Bronze::LAYER, Layer::Bronze);
        assert_eq!(layers::Silver::LAYER, Layer::Silver);
        assert_eq!(layers::Gold::LAYER, Layer::Gold);

        fn replaceable<L: Replaceable>() -> Layer {
            L::LAYER
        }
        for layer in [
            replaceable::<layers::Silver>(),
            replaceable::<layers::Gold>(),
        ] {
            assert!(layer.permits_replacement());
        }
    }

    #[test]
    fn directory_names_are_the_layer_names() {
        assert_eq!(Layer::Landing.as_str(), "landing");
        assert_eq!(Layer::Bronze.as_str(), "bronze");
        assert_eq!(Layer::Silver.as_str(), "silver");
        assert_eq!(Layer::Gold.as_str(), "gold");
    }
}
