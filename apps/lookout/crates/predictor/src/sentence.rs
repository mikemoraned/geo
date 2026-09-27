use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SentenceError {
    #[error("does not start with `$`")]
    NoStart,
    #[error("carries no `*` checksum")]
    NoChecksum,
    #[error("ends in something other than two hex digits")]
    NotHexChecksum,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sentence(String);

impl Sentence {
    pub fn new(sentence: impl Into<String>) -> Result<Self, SentenceError> {
        let sentence = sentence.into().trim().to_string();
        let checksum = sentence
            .strip_prefix('$')
            .ok_or(SentenceError::NoStart)?
            .rsplit_once('*')
            .ok_or(SentenceError::NoChecksum)?
            .1;

        match checksum.len() == 2 && checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            true => Ok(Self(sentence)),
            false => Err(SentenceError::NotHexChecksum),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn body(&self) -> &str {
        let body = self.0.strip_prefix('$').unwrap_or(&self.0);

        body.rsplit_once('*').map_or(body, |(body, _)| body)
    }
}

impl FromStr for Sentence {
    type Err = SentenceError;

    fn from_str(sentence: &str) -> Result<Self, Self::Err> {
        Self::new(sentence)
    }
}

impl TryFrom<String> for Sentence {
    type Error = SentenceError;

    fn try_from(sentence: String) -> Result<Self, Self::Error> {
        Self::new(sentence)
    }
}

impl From<Sentence> for String {
    fn from(sentence: Sentence) -> Self {
        sentence.0
    }
}

impl fmt::Display for Sentence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIX: &str = "$GNGGA,204329.00,5030.00000,N,00830.00000,E,1,06,4.4,262.46,M,45.12,M,,*63";

    #[test]
    fn a_sentence_keeps_what_it_was_given() {
        let sentence = Sentence::new(FIX).expect("a sentence");

        assert_eq!(sentence.as_str(), FIX);
        assert_eq!(sentence.to_string(), FIX);
    }

    #[test]
    fn a_body_is_the_fields_between_the_markers() {
        let sentence = Sentence::new("$GPTXT,01,01,01,ANTENNA OPEN*25").expect("a sentence");

        assert_eq!(sentence.body(), "GPTXT,01,01,01,ANTENNA OPEN");
    }

    #[test]
    fn a_line_off_a_uart_needs_no_trimming() {
        let sentence = Sentence::new(format!("{FIX}\r\n")).expect("a sentence");

        assert_eq!(sentence.as_str(), FIX);
    }

    #[test]
    fn noise_on_the_line_is_not_a_sentence() {
        assert_eq!(
            Sentence::new("\0\u{1}not a sentence"),
            Err(SentenceError::NoStart)
        );
        assert_eq!(
            Sentence::new("$GNGGA,204329.00"),
            Err(SentenceError::NoChecksum)
        );
        assert_eq!(
            Sentence::new("$GNGGA,204329.00*zz"),
            Err(SentenceError::NotHexChecksum)
        );
        assert_eq!(
            Sentence::new("$GNGGA,204329.00*6"),
            Err(SentenceError::NotHexChecksum)
        );
    }

    #[test]
    fn a_sentence_whose_checksum_is_wrong_is_still_a_sentence() {
        assert!(Sentence::new("$GAGSV,12724.00,V,N*55").is_ok());
    }

    #[test]
    fn a_sentence_parses_from_a_string() {
        assert_eq!(FIX.parse::<Sentence>().expect("a sentence").as_str(), FIX);
        assert!("nonsense".parse::<Sentence>().is_err());
    }

    #[test]
    fn a_sentence_survives_a_round_trip() {
        let sentence = Sentence::new(FIX).expect("a sentence");

        let json = serde_json::to_string(&sentence).expect("serialise");

        assert_eq!(
            serde_json::from_str::<Sentence>(&json).expect("deserialise"),
            sentence
        );
        assert!(serde_json::from_str::<Sentence>("\"nonsense\"").is_err());
    }
}
