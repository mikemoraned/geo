use std::fmt;
use std::str::FromStr;

use base32::Alphabet;
use serde::{Deserialize, Serialize};

const LENGTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ShortCode(String);

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a short code of 8 Crockford base32 characters")]
pub struct NotAShortCode(String);

impl ShortCode {
    pub fn random() -> Self {
        Self::from_bytes(rand::random())
    }

    fn from_bytes(bytes: [u8; 5]) -> Self {
        Self(base32::encode(Alphabet::Crockford, &bytes))
    }

    pub fn new(typed: impl Into<String>) -> Result<Self, NotAShortCode> {
        let typed = typed.into();
        let compact: String = typed
            .chars()
            .filter(|c| *c != '-' && !c.is_whitespace())
            .collect();
        let bytes = (compact.chars().count() == LENGTH)
            .then(|| base32::decode(Alphabet::Crockford, &compact))
            .flatten()
            .and_then(|bytes| <[u8; 5]>::try_from(bytes).ok());
        bytes.map(Self::from_bytes).ok_or(NotAShortCode(typed))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ShortCode {
    type Err = NotAShortCode;

    fn from_str(typed: &str) -> Result<Self, Self::Err> {
        Self::new(typed)
    }
}

impl TryFrom<String> for ShortCode {
    type Error = NotAShortCode;

    fn try_from(typed: String) -> Result<Self, Self::Error> {
        Self::new(typed)
    }
}

impl From<ShortCode> for String {
    fn from(code: ShortCode) -> Self {
        code.0
    }
}

impl fmt::Display for ShortCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (first, second) = self.0.split_at(LENGTH / 2);
        write!(f, "{first}-{second}")
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const CROCKFORD: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

    #[test]
    fn a_random_code_is_eight_crockford_characters() {
        let code = ShortCode::random();

        assert_eq!(code.as_str().len(), 8);
        assert!(code.as_str().chars().all(|c| CROCKFORD.contains(c)));
    }

    #[test]
    fn a_code_shows_in_two_groups_of_four() {
        let code = ShortCode::new("K7Q2M9XD").expect("a code");

        assert_eq!(code.to_string(), "K7Q2-M9XD");
    }

    #[test]
    fn typing_ignores_case_hyphens_and_spaces() {
        assert_eq!(ShortCode::new(" k7q2-m9xd "), ShortCode::new("K7Q2M9XD"));
    }

    #[test]
    fn typing_reads_letters_that_look_like_digits_as_those_digits() {
        assert_eq!(ShortCode::new("OILQ-M9XD"), ShortCode::new("011Q-M9XD"));
    }

    #[test]
    fn typing_refuses_the_wrong_length_or_alphabet() {
        for typed in ["", "K7Q2M9X", "K7Q2M9XDZ", "K7Q2M9XU", "K7Q2!9XD"] {
            assert!(ShortCode::new(typed).is_err(), "{typed:?} was accepted");
        }
    }

    proptest! {
        #[test]
        fn a_shown_code_types_back_to_itself(bytes in any::<[u8; 5]>(), lower in any::<bool>()) {
            let code = ShortCode::from_bytes(bytes);
            let shown = code.to_string();
            let typed = if lower { shown.to_lowercase() } else { shown };

            prop_assert_eq!(ShortCode::new(typed), Ok(code));
        }
    }
}
