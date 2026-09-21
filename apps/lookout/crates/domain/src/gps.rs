//! A GPS fix, as whatever produced it knew it.

use serde::{Deserialize, Serialize};

/// One fix: where, how well, and how fast.
///
/// Every field but the position is optional or nullable, because sources disagree about what
/// a fix comes with. A phone reports accuracy in metres and nothing about satellites; it
/// reports no heading when standing still, and a null there means "not moving" rather than
/// "unknown", so the nulls are kept rather than dropped.
///
/// **Recordings abbreviate these names and the code does not.** A browser has sent the short
/// names since the first recording and the archive still holds them, so they are fixed.
/// Renaming a field here means adjusting its `rename` to leave the stored shape alone;
/// renaming that strands every fix already recorded.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Gps {
    #[serde(rename = "lat")]
    pub latitude: f64,
    #[serde(rename = "lon")]
    pub longitude: f64,
    #[serde(rename = "alt")]
    pub altitude_metres: Option<f64>,
    /// How far out the fix is, in metres, as the source judged it.
    #[serde(rename = "acc")]
    pub accuracy_metres: f64,
    /// Metres per second, Doppler-derived where the source can.
    #[serde(rename = "speed", default)]
    pub speed_mps: Option<f64>,
    /// Course over ground, degrees clockwise from true north. Null when standing still, since
    /// there is no course to report.
    #[serde(rename = "heading", default)]
    pub heading_degrees: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix() -> Gps {
        Gps {
            latitude: 55.95,
            longitude: -3.19,
            altitude_metres: None,
            accuracy_metres: 8.5,
            speed_mps: Some(31.4),
            heading_degrees: None,
        }
    }

    /// The names an archive of recordings is written in. A rename here would strand every fix
    /// already stored.
    #[test]
    fn a_fix_is_written_in_the_names_the_wire_uses() {
        let json = serde_json::to_string(&fix()).expect("serialize");

        assert_eq!(
            json,
            r#"{"lat":55.95,"lon":-3.19,"alt":null,"acc":8.5,"speed":31.4,"heading":null}"#
        );
    }

    /// The oldest recordings carry neither speed nor heading, and are still read.
    #[test]
    fn a_fix_from_before_speed_and_heading_were_sent_still_reads() {
        let v0 = r#"{"lat":55.95,"lon":-3.19,"alt":null,"acc":8.5}"#;

        let fix: Gps = serde_json::from_str(v0).expect("deserialize");

        assert_eq!(fix.speed_mps, None);
        assert_eq!(fix.heading_degrees, None);
    }

    #[test]
    fn a_fix_survives_the_wire() {
        let json = serde_json::to_string(&fix()).expect("serialize");

        assert_eq!(
            serde_json::from_str::<Gps>(&json).expect("deserialize"),
            fix()
        );
    }
}
