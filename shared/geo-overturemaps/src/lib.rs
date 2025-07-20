use std::fmt::Display;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct GersId(String);

impl GersId {
    pub fn new(id: String) -> Self {
        GersId(id)
    }
}

impl Display for GersId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
