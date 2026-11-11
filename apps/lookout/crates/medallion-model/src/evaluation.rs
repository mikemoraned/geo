use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use domain::{CrossingCompactId, Percentage, SessionId, StateKind};
use medallion::{DatasetSpec, RUN_DATE, RUN_ID, Row, layers};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumIter, EnumString};

use crate::TrainSessionRowV3;

pub const EVALUATION_RUN_V4: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned_within("evaluation_run_v4", RUN_DATE, &[RUN_ID]);

pub const EVALUATION_SESSION_V1: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned_within("evaluation_session_v1", RUN_DATE, &[RUN_ID]);

pub const EVALUATION_TRANSITION_V1: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned_within("evaluation_transition_v1", RUN_DATE, &[RUN_ID]);

pub const EVALUATION_OUTCOME_V1: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned_within("evaluation_outcome_v1", RUN_DATE, &[RUN_ID]);

pub const EVALUATION_SUMMARY_V1: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned_within("evaluation_summary_v1", RUN_DATE, &[RUN_ID]);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationTransitionRowV1 {
    pub session_id: SessionId,
    pub crossing_compact_id: CrossingCompactId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub at: DateTime<Utc>,
    pub from: StateKind,
    pub to: StateKind,
}

impl Row for EvaluationTransitionRowV1 {
    type Layer = layers::Gold;
    const DATASET: DatasetSpec<Self::Layer> = EVALUATION_TRANSITION_V1;
    const INSTANTS: &'static [&'static str] = &["at"];

    fn example() -> Self {
        Self {
            session_id: TrainSessionRowV3::example().session_id,
            crossing_compact_id: CrossingCompactId::new(1),
            at: DateTime::UNIX_EPOCH,
            from: StateKind::OutOfReach,
            to: StateKind::OutOfReach,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeKind {
    Credited,
    Repeat,
    FalseAlarm,
    Unscored,
    Miss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationClass {
    Hit,
    LastMoment,
    TooEarly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationOutcomeRowV1 {
    pub session_id: SessionId,
    pub crossing_compact_id: CrossingCompactId,
    pub outcome: OutcomeKind,
    #[serde(with = "chrono::serde::ts_milliseconds_option")]
    pub notified_at: Option<DateTime<Utc>>,
    #[serde(with = "chrono::serde::ts_milliseconds_option")]
    pub crossed_at: Option<DateTime<Utc>>,
    pub lead_ms: Option<i64>,
    pub class: Option<NotificationClass>,
    pub score: Option<f64>,
}

impl Row for EvaluationOutcomeRowV1 {
    type Layer = layers::Gold;
    const DATASET: DatasetSpec<Self::Layer> = EVALUATION_OUTCOME_V1;
    const INSTANTS: &'static [&'static str] = &["notified_at", "crossed_at"];

    fn example() -> Self {
        Self {
            session_id: TrainSessionRowV3::example().session_id,
            crossing_compact_id: CrossingCompactId::new(1),
            outcome: OutcomeKind::Credited,
            notified_at: Some(DateTime::UNIX_EPOCH),
            crossed_at: Some(DateTime::UNIX_EPOCH),
            lead_ms: Some(0),
            class: Some(NotificationClass::Hit),
            score: Some(1.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationSummaryRowV1 {
    pub sessions: u64,
    pub hours: f64,
    pub passes: u64,
    pub notifications: u64,
    pub false_alarms: u64,
    pub repeats: u64,
    pub misses: u64,
    pub precision: Option<f64>,
    pub precision_low: Option<f64>,
    pub precision_high: Option<f64>,
    pub recall: Option<f64>,
    pub recall_low: Option<f64>,
    pub recall_high: Option<f64>,
    pub false_alarms_per_hour: Option<f64>,
    pub false_alarms_per_hour_low: Option<f64>,
    pub false_alarms_per_hour_high: Option<f64>,
    pub lead_p10_ms: Option<i64>,
    pub lead_median_ms: Option<i64>,
    pub lead_p90_ms: Option<i64>,
}

impl Row for EvaluationSummaryRowV1 {
    type Layer = layers::Gold;
    const DATASET: DatasetSpec<Self::Layer> = EVALUATION_SUMMARY_V1;

    fn example() -> Self {
        Self {
            sessions: 1,
            hours: 1.0,
            passes: 1,
            notifications: 1,
            false_alarms: 0,
            repeats: 0,
            misses: 0,
            precision: Some(1.0),
            precision_low: Some(1.0),
            precision_high: Some(1.0),
            recall: Some(1.0),
            recall_low: Some(1.0),
            recall_high: Some(1.0),
            false_alarms_per_hour: Some(0.0),
            false_alarms_per_hour_low: Some(0.0),
            false_alarms_per_hour_high: Some(0.0),
            lead_p10_ms: Some(300_000),
            lead_median_ms: Some(300_000),
            lead_p90_ms: Some(300_000),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumString, AsRefStr, EnumIter,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PredictorName {
    Oracle,
    CrowFlies,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumString, AsRefStr, EnumIter,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum FloatType {
    F32,
    F64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationRunRowV4 {
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub started_at: DateTime<Utc>,
    pub evaluation_version: String,
    pub code_version: String,
    pub country: String,
    pub realtime_at_least: Percentage,
    pub sessions: u64,
    pub pick_seed: u64,
    pub predictor: PredictorName,
    pub predictor_settings: BTreeMap<String, String>,
    pub online_detector_settings: BTreeMap<String, String>,
    pub float_type: FloatType,
    pub horizon_ms: i64,
    pub last_moment_ms: i64,
    pub ramp_ms: i64,
    pub last_moment_score: f64,
    pub floor: f64,
    pub warm_up_ms: i64,
    pub resamples: u64,
    pub bootstrap_seed: u64,
    pub fingerprints: BTreeMap<String, String>,
}

impl Row for EvaluationRunRowV4 {
    type Layer = layers::Gold;
    const DATASET: DatasetSpec<Self::Layer> = EVALUATION_RUN_V4;
    const INSTANTS: &'static [&'static str] = &["started_at"];

    fn example() -> Self {
        Self {
            started_at: DateTime::UNIX_EPOCH,
            evaluation_version: "replay-v1".to_string(),
            code_version: "0000000000000000000000000000000000000000".to_string(),
            country: "DE".to_string(),
            realtime_at_least: Percentage::new(90.0).expect("90 is a percentage"),
            sessions: 1000,
            pick_seed: 1,
            predictor: PredictorName::Oracle,
            predictor_settings: BTreeMap::from([("horizon".to_string(), "5m".to_string())]),
            online_detector_settings: BTreeMap::from([(
                "pass_radius_metres".to_string(),
                "100".to_string(),
            )]),
            float_type: FloatType::F32,
            horizon_ms: 300_000,
            last_moment_ms: 30_000,
            ramp_ms: 60_000,
            last_moment_score: 0.25,
            floor: 0.05,
            warm_up_ms: 60_000,
            resamples: 1000,
            bootstrap_seed: 1,
            fingerprints: BTreeMap::from([("example".to_string(), "example".to_string())]),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationSessionRowV1 {
    pub session_id: SessionId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub started_at: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub ended_at: DateTime<Utc>,
    pub hours: f64,
}

impl Row for EvaluationSessionRowV1 {
    type Layer = layers::Gold;
    const DATASET: DatasetSpec<Self::Layer> = EVALUATION_SESSION_V1;
    const INSTANTS: &'static [&'static str] = &["started_at", "ended_at"];

    fn example() -> Self {
        Self {
            session_id: TrainSessionRowV3::example().session_id,
            started_at: DateTime::UNIX_EPOCH,
            ended_at: DateTime::UNIX_EPOCH,
            hours: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;
    use std::str::FromStr;

    use strum::IntoEnumIterator;

    use super::*;

    fn spelt_alike<E>()
    where
        E: IntoEnumIterator + AsRef<str> + FromStr + Serialize + Debug + PartialEq,
        E::Err: Debug,
    {
        for variant in E::iter() {
            let stored = serde_json::to_value(&variant).expect("a stored name");
            assert_eq!(stored, serde_json::json!(variant.as_ref()), "{variant:?}");
            assert_eq!(
                E::from_str(variant.as_ref()).expect("a parsed name"),
                variant
            );
        }
    }

    #[test]
    fn a_predictor_is_named_as_it_is_stored() {
        spelt_alike::<PredictorName>();
    }

    #[test]
    fn a_float_type_is_named_as_it_is_stored() {
        spelt_alike::<FloatType>();
    }
}
