use std::collections::BTreeMap;
use std::fmt::Display;
use std::str::FromStr;

use domain::{Radius, Span};
use medallion_model::PredictorName;

use crate::steps::Predictor;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SettingsError {
    #[error("the predictor needs the setting `{0}`")]
    Missing(&'static str),
    #[error("the setting `{key}` cannot be read: {reason}")]
    Unreadable { key: &'static str, reason: String },
    #[error("the predictor takes no settings named {0:?}")]
    Unknown(Vec<String>),
}

pub trait TypedSettings: Sized {
    fn write(&self) -> BTreeMap<String, String>;
    fn read(settings: &BTreeMap<String, String>) -> Result<Self, SettingsError>;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OracleSettings {
    pub horizon: Span,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrowFliesSettings {
    pub radius: Radius,
    pub horizon: Span,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OnlineDetectorSettings {
    pub pass_radius: Radius,
}

impl TypedSettings for OracleSettings {
    fn write(&self) -> BTreeMap<String, String> {
        BTreeMap::from([("horizon".to_string(), self.horizon.to_string())])
    }

    fn read(settings: &BTreeMap<String, String>) -> Result<Self, SettingsError> {
        let mut reading = Reading::new(settings);
        let read = Self {
            horizon: reading.take("horizon")?,
        };
        reading.finish()?;
        Ok(read)
    }
}

impl TypedSettings for CrowFliesSettings {
    fn write(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("radius_metres".to_string(), self.radius.to_string()),
            ("horizon".to_string(), self.horizon.to_string()),
        ])
    }

    fn read(settings: &BTreeMap<String, String>) -> Result<Self, SettingsError> {
        let mut reading = Reading::new(settings);
        let read = Self {
            radius: reading.take("radius_metres")?,
            horizon: reading.take("horizon")?,
        };
        reading.finish()?;
        Ok(read)
    }
}

impl TypedSettings for OnlineDetectorSettings {
    fn write(&self) -> BTreeMap<String, String> {
        BTreeMap::from([(
            "pass_radius_metres".to_string(),
            self.pass_radius.to_string(),
        )])
    }

    fn read(settings: &BTreeMap<String, String>) -> Result<Self, SettingsError> {
        let mut reading = Reading::new(settings);
        let read = Self {
            pass_radius: reading.take("pass_radius_metres")?,
        };
        reading.finish()?;
        Ok(read)
    }
}

pub fn write_online_detector(
    online_detector: Option<OnlineDetectorSettings>,
) -> BTreeMap<String, String> {
    online_detector
        .map(|online_detector| online_detector.write())
        .unwrap_or_default()
}

pub fn read_online_detector(
    settings: &BTreeMap<String, String>,
) -> Result<Option<OnlineDetectorSettings>, SettingsError> {
    (!settings.is_empty())
        .then(|| OnlineDetectorSettings::read(settings))
        .transpose()
}

impl Predictor {
    pub fn name(&self) -> PredictorName {
        match self {
            Predictor::Oracle(_) => PredictorName::Oracle,
            Predictor::CrowFlies(_) => PredictorName::CrowFlies,
        }
    }

    pub fn settings(&self) -> BTreeMap<String, String> {
        match self {
            Predictor::Oracle(settings) => settings.write(),
            Predictor::CrowFlies(settings) => settings.write(),
        }
    }

    pub fn read(
        name: PredictorName,
        settings: &BTreeMap<String, String>,
    ) -> Result<Self, SettingsError> {
        Ok(match name {
            PredictorName::Oracle => Predictor::Oracle(OracleSettings::read(settings)?),
            PredictorName::CrowFlies => Predictor::CrowFlies(CrowFliesSettings::read(settings)?),
        })
    }
}

struct Reading<'a> {
    settings: &'a BTreeMap<String, String>,
    read: Vec<&'static str>,
}

impl<'a> Reading<'a> {
    fn new(settings: &'a BTreeMap<String, String>) -> Self {
        Self {
            settings,
            read: Vec::new(),
        }
    }

    fn take<T>(&mut self, key: &'static str) -> Result<T, SettingsError>
    where
        T: FromStr,
        T::Err: Display,
    {
        self.read.push(key);
        let text = self.settings.get(key).ok_or(SettingsError::Missing(key))?;
        text.parse()
            .map_err(|err: T::Err| SettingsError::Unreadable {
                key,
                reason: err.to_string(),
            })
    }

    fn finish(self) -> Result<(), SettingsError> {
        let unknown: Vec<String> = self
            .settings
            .keys()
            .filter(|key| !self.read.contains(&key.as_str()))
            .cloned()
            .collect();
        if unknown.is_empty() {
            Ok(())
        } else {
            Err(SettingsError::Unknown(unknown))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    fn crow_flies() -> Predictor {
        Predictor::CrowFlies(CrowFliesSettings {
            radius: Radius::new(1_000.0).expect("a radius"),
            horizon: Span::new("5m").expect("a span"),
        })
    }

    fn oracle() -> Predictor {
        Predictor::Oracle(OracleSettings {
            horizon: Span::new("90s").expect("a span"),
        })
    }

    #[test]
    fn crow_flies_writes_its_radius_in_metres_and_its_horizon_as_a_span() {
        assert_eq!(
            (crow_flies().name(), crow_flies().settings()),
            (
                PredictorName::CrowFlies,
                map(&[("horizon", "5m"), ("radius_metres", "1000")])
            )
        );
    }

    #[test]
    fn the_oracle_writes_its_horizon_alone() {
        assert_eq!(
            (oracle().name(), oracle().settings()),
            (PredictorName::Oracle, map(&[("horizon", "1m 30s")]))
        );
    }

    #[test]
    fn each_predictors_settings_survive_a_round_trip_through_the_map() {
        for predictor in [oracle(), crow_flies()] {
            assert_eq!(
                Predictor::read(predictor.name(), &predictor.settings()),
                Ok(predictor)
            );
        }
    }

    fn online_detector() -> OnlineDetectorSettings {
        OnlineDetectorSettings {
            pass_radius: Radius::new(100.0).expect("a radius"),
        }
    }

    #[test]
    fn the_online_detector_writes_its_pass_radius_in_metres() {
        assert_eq!(
            write_online_detector(Some(online_detector())),
            map(&[("pass_radius_metres", "100")])
        );
    }

    #[test]
    fn a_run_without_an_online_detector_writes_an_empty_map() {
        assert_eq!(write_online_detector(None), BTreeMap::new());
    }

    #[test]
    fn the_online_detectors_settings_survive_a_round_trip_through_the_map() {
        for online_detector in [None, Some(online_detector())] {
            assert_eq!(
                read_online_detector(&write_online_detector(online_detector)),
                Ok(online_detector)
            );
        }
    }

    #[test]
    fn online_detector_settings_without_a_pass_radius_are_refused() {
        assert_eq!(
            read_online_detector(&map(&[("radius_metres", "100")])),
            Err(SettingsError::Missing("pass_radius_metres"))
        );
    }

    #[test]
    fn a_missing_setting_is_refused_by_name() {
        assert_eq!(
            Predictor::read(PredictorName::CrowFlies, &map(&[("horizon", "5m")])),
            Err(SettingsError::Missing("radius_metres"))
        );
    }

    #[test]
    fn a_setting_another_predictor_takes_is_refused() {
        assert_eq!(
            Predictor::read(
                PredictorName::Oracle,
                &map(&[("horizon", "5m"), ("radius_metres", "1000")])
            ),
            Err(SettingsError::Unknown(vec!["radius_metres".to_string()]))
        );
    }

    #[test]
    fn a_setting_that_does_not_parse_is_refused_with_its_reason() {
        assert!(matches!(
            Predictor::read(
                PredictorName::CrowFlies,
                &map(&[("horizon", "5m"), ("radius_metres", "far")])
            ),
            Err(SettingsError::Unreadable {
                key: "radius_metres",
                ..
            })
        ));
    }
}
