//! The layers data flows through, and the directory each occupies under the root.

/// A layer of the store. See `docs/medallion.md` for what belongs in each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Landing,
    Bronze,
    Silver,
    Gold,
}

impl Layer {
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

    #[test]
    fn directory_names_are_the_layer_names() {
        assert_eq!(Layer::Landing.as_str(), "landing");
        assert_eq!(Layer::Bronze.as_str(), "bronze");
        assert_eq!(Layer::Silver.as_str(), "silver");
        assert_eq!(Layer::Gold.as_str(), "gold");
    }
}
