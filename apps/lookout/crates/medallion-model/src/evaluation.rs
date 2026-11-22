use std::collections::BTreeMap;

use chrono::{DateTime, TimeDelta, Utc};
use domain::{CrossingCompactId, Percentage, SessionId, StateKind};
use medallion::{DatasetSpec, RUN_DATE, RUN_ID, Row, layers};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumIter, EnumString};

use crate::TrainSessionRowV3;

pub const EVALUATION_RUN_V6: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned_within("evaluation_run_v6", RUN_DATE, &[RUN_ID]);

pub const EVALUATION_SESSION_V3: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned_within("evaluation_session_v3", RUN_DATE, &[RUN_ID]);

pub const EVALUATION_TRANSITION_V1: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned_within("evaluation_transition_v1", RUN_DATE, &[RUN_ID]);

pub const EVALUATION_OUTCOME_V2: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned_within("evaluation_outcome_v2", RUN_DATE, &[RUN_ID]);

pub const EVALUATION_OUTCOME_V3: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned_within("evaluation_outcome_v3", RUN_DATE, &[RUN_ID]);

pub const EVALUATION_SUMMARY_V3: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned_within("evaluation_summary_v3", RUN_DATE, &[RUN_ID]);

pub const EVALUATION_SKEW_V2: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned_within("evaluation_skew_v2", RUN_DATE, &[RUN_ID]);

pub const EVALUATION_SPLIT_V1: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned_within("evaluation_split_v1", RUN_DATE, &[RUN_ID]);

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
pub struct EvaluationOutcomeRowV2 {
    pub session_id: SessionId,
    pub crossing_compact_id: CrossingCompactId,
    pub outcome: OutcomeKind,
    #[serde(with = "chrono::serde::ts_milliseconds_option")]
    pub notified_at: Option<DateTime<Utc>>,
    #[serde(with = "chrono::serde::ts_milliseconds_option")]
    pub crossed_at: Option<DateTime<Utc>>,
    #[serde(with = "medallion::duration::milliseconds_option")]
    pub lead: Option<TimeDelta>,
    pub class: Option<NotificationClass>,
    pub score: Option<f64>,
}

impl Row for EvaluationOutcomeRowV2 {
    type Layer = layers::Gold;
    const DATASET: DatasetSpec<Self::Layer> = EVALUATION_OUTCOME_V2;
    const INSTANTS: &'static [&'static str] = &["notified_at", "crossed_at"];
    const DURATIONS: &'static [&'static str] = &["lead"];

    fn example() -> Self {
        Self {
            session_id: TrainSessionRowV3::example().session_id,
            crossing_compact_id: CrossingCompactId::new(1),
            outcome: OutcomeKind::Credited,
            notified_at: Some(DateTime::UNIX_EPOCH),
            crossed_at: Some(DateTime::UNIX_EPOCH),
            lead: Some(TimeDelta::zero()),
            class: Some(NotificationClass::Hit),
            score: Some(1.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationOutcomeRowV3 {
    pub session_index: u32,
    pub crossing_compact_id: CrossingCompactId,
    pub outcome: OutcomeKind,
    pub class: Option<NotificationClass>,
    pub score: Option<f64>,
    pub notified_offset_ms: Option<u32>,
    pub crossed_offset_ms: Option<u32>,
}

impl Row for EvaluationOutcomeRowV3 {
    type Layer = layers::Gold;
    const DATASET: DatasetSpec<Self::Layer> = EVALUATION_OUTCOME_V3;

    fn example() -> Self {
        Self {
            session_index: 0,
            crossing_compact_id: CrossingCompactId::new(1),
            outcome: OutcomeKind::Credited,
            class: Some(NotificationClass::Hit),
            score: Some(1.0),
            notified_offset_ms: Some(0),
            crossed_offset_ms: Some(300_000),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationSummaryRowV3 {
    pub sessions: u64,
    #[serde(with = "medallion::duration::milliseconds")]
    pub scored_duration: TimeDelta,
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
    pub f_half: Option<f64>,
    pub f_half_low: Option<f64>,
    pub f_half_high: Option<f64>,
    #[serde(with = "medallion::duration::milliseconds_option")]
    pub lead_p10: Option<TimeDelta>,
    #[serde(with = "medallion::duration::milliseconds_option")]
    pub lead_median: Option<TimeDelta>,
    #[serde(with = "medallion::duration::milliseconds_option")]
    pub lead_p90: Option<TimeDelta>,
}

impl Row for EvaluationSummaryRowV3 {
    type Layer = layers::Gold;
    const DATASET: DatasetSpec<Self::Layer> = EVALUATION_SUMMARY_V3;
    const DURATIONS: &'static [&'static str] =
        &["scored_duration", "lead_p10", "lead_median", "lead_p90"];

    fn example() -> Self {
        Self {
            sessions: 1,
            scored_duration: TimeDelta::hours(1),
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
            f_half: Some(1.0),
            f_half_low: Some(1.0),
            f_half_high: Some(1.0),
            lead_p10: Some(TimeDelta::minutes(5)),
            lead_median: Some(TimeDelta::minutes(5)),
            lead_p90: Some(TimeDelta::minutes(5)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationSkewRowV2 {
    pub sessions: u64,
    pub passes: u64,
    pub visits: u64,
    pub seen_passes: u64,
    pub unmatched_visits: u64,
    pub seen: Option<f64>,
    pub seen_low: Option<f64>,
    pub seen_high: Option<f64>,
    pub unmatched: Option<f64>,
    pub unmatched_low: Option<f64>,
    pub unmatched_high: Option<f64>,
    #[serde(with = "medallion::duration::milliseconds_option")]
    pub offset_p10: Option<TimeDelta>,
    #[serde(with = "medallion::duration::milliseconds_option")]
    pub offset_median: Option<TimeDelta>,
    #[serde(with = "medallion::duration::milliseconds_option")]
    pub offset_p90: Option<TimeDelta>,
}

impl Row for EvaluationSkewRowV2 {
    type Layer = layers::Gold;
    const DATASET: DatasetSpec<Self::Layer> = EVALUATION_SKEW_V2;
    const DURATIONS: &'static [&'static str] = &["offset_p10", "offset_median", "offset_p90"];

    fn example() -> Self {
        Self {
            sessions: 1,
            passes: 1,
            visits: 1,
            seen_passes: 1,
            unmatched_visits: 0,
            seen: Some(1.0),
            seen_low: Some(1.0),
            seen_high: Some(1.0),
            unmatched: Some(0.0),
            unmatched_low: Some(0.0),
            unmatched_high: Some(0.0),
            offset_p10: Some(TimeDelta::seconds(-10)),
            offset_median: Some(TimeDelta::seconds(-5)),
            offset_p90: Some(TimeDelta::zero()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationSplitRowV1 {
    pub cells: u64,
    pub cells_in_split: u64,
    pub crossings: u64,
    pub crossings_in_split: u64,
    pub passes_in_split: u64,
    pub passes_outside: u64,
    pub outcomes_in_split: u64,
    pub outcomes_outside: u64,
    pub cells_outside_cover: u64,
}

impl Row for EvaluationSplitRowV1 {
    type Layer = layers::Gold;
    const DATASET: DatasetSpec<Self::Layer> = EVALUATION_SPLIT_V1;

    fn example() -> Self {
        Self {
            cells: 2,
            cells_in_split: 1,
            crossings: 2,
            crossings_in_split: 1,
            passes_in_split: 2,
            passes_outside: 1,
            outcomes_in_split: 2,
            outcomes_outside: 1,
            cells_outside_cover: 0,
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
    LightCone,
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
pub struct EvaluationRunRowV6 {
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
    pub split_settings: BTreeMap<String, String>,
    pub float_type: FloatType,
    #[serde(with = "medallion::duration::milliseconds")]
    pub horizon: TimeDelta,
    #[serde(with = "medallion::duration::milliseconds")]
    pub last_moment: TimeDelta,
    #[serde(with = "medallion::duration::milliseconds")]
    pub ramp: TimeDelta,
    pub last_moment_score: f64,
    pub floor: f64,
    #[serde(with = "medallion::duration::milliseconds")]
    pub warm_up: TimeDelta,
    pub resamples: u64,
    pub bootstrap_seed: u64,
    pub fingerprints: BTreeMap<String, String>,
}

impl Row for EvaluationRunRowV6 {
    type Layer = layers::Gold;
    const DATASET: DatasetSpec<Self::Layer> = EVALUATION_RUN_V6;
    const INSTANTS: &'static [&'static str] = &["started_at"];
    const DURATIONS: &'static [&'static str] = &["horizon", "last_moment", "ramp", "warm_up"];

    fn example() -> Self {
        Self {
            started_at: DateTime::UNIX_EPOCH,
            evaluation_version: "replay-v1".to_string(),
            code_version: "0000000000000000000000000000000000000000".to_string(),
            country: "DE".to_string(),
            realtime_at_least: const { Percentage::checked(90.0).expect("90 lies from 0 to 100") },
            sessions: 1000,
            pick_seed: 1,
            predictor: PredictorName::Oracle,
            predictor_settings: BTreeMap::from([("horizon".to_string(), "5m".to_string())]),
            online_detector_settings: BTreeMap::from([(
                "pass_radius_metres".to_string(),
                "100".to_string(),
            )]),
            split_settings: BTreeMap::from([("split".to_string(), "test".to_string())]),
            float_type: FloatType::F32,
            horizon: TimeDelta::minutes(5),
            last_moment: TimeDelta::seconds(30),
            ramp: TimeDelta::minutes(1),
            last_moment_score: 0.25,
            floor: 0.05,
            warm_up: TimeDelta::minutes(1),
            resamples: 1000,
            bootstrap_seed: 1,
            fingerprints: BTreeMap::from([("example".to_string(), "example".to_string())]),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationSessionRowV3 {
    pub session_index: u32,
    pub session_id: SessionId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub started_at: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub ended_at: DateTime<Utc>,
    #[serde(with = "medallion::duration::milliseconds")]
    pub duration: TimeDelta,
    #[serde(with = "medallion::duration::milliseconds")]
    pub scored_duration: TimeDelta,
}

impl Row for EvaluationSessionRowV3 {
    type Layer = layers::Gold;
    const DATASET: DatasetSpec<Self::Layer> = EVALUATION_SESSION_V3;
    const INSTANTS: &'static [&'static str] = &["started_at", "ended_at"];
    const DURATIONS: &'static [&'static str] = &["duration", "scored_duration"];

    fn example() -> Self {
        Self {
            session_index: 0,
            session_id: TrainSessionRowV3::example().session_id,
            started_at: DateTime::UNIX_EPOCH,
            ended_at: DateTime::UNIX_EPOCH,
            duration: TimeDelta::zero(),
            scored_duration: TimeDelta::zero(),
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
