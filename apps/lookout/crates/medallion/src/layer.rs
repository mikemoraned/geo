#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Landing,
    Bronze,
    Silver,
    Gold,
}

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

pub trait LayerKind: Copy + std::fmt::Debug + PartialEq + Eq + std::hash::Hash {
    const LAYER: Layer;
}

pub trait Replaceable: LayerKind {}

impl Layer {
    pub fn permits_replacement(self) -> bool {
        matches!(self, Layer::Silver | Layer::Gold)
    }

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

    #[test]
    fn only_the_derived_layers_permit_replacement() {
        assert!(!Layer::Landing.permits_replacement());
        assert!(!Layer::Bronze.permits_replacement());
        assert!(Layer::Silver.permits_replacement());
        assert!(Layer::Gold.permits_replacement());
    }

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
