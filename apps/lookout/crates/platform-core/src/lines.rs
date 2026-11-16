use serde::{Deserialize, Serialize};
use serde_with::base64::Base64;
use serde_with::serde_as;

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wkb(#[serde_as(as = "Base64")] Vec<u8>);

impl Wkb {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}
