use chrono::{DateTime, Utc};
use domain::{CrossingCompactId, Percentage, SessionId};
use medallion::{DatasetSpec, RUN_DATE, Row, layers};
use serde::{Deserialize, Serialize};

use crate::TrainSessionRowV3;

pub const EVALUATION_RUN_V1: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned("evaluation_run_v1", RUN_DATE);

pub const EVALUATION_SESSION_V1: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned("evaluation_session_v1", RUN_DATE);

pub const EVALUATION_TRANSITION_V1: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned("evaluation_transition_v1", RUN_DATE);

pub const EVALUATION_OUTCOME_V1: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned("evaluation_outcome_v1", RUN_DATE);

pub const EVALUATION_SUMMARY_V1: DatasetSpec<layers::Gold> =
    DatasetSpec::partitioned("evaluation_summary_v1", RUN_DATE);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossingStateKind {
    OutOfReach,
    CandidatePass,
    AlertedPass,
    ActualPass,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationTransitionRowV1 {
    pub session_id: SessionId,
    pub crossing_compact_id: CrossingCompactId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub at: DateTime<Utc>,
    pub from: CrossingStateKind,
    pub to: CrossingStateKind,
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
            from: CrossingStateKind::OutOfReach,
            to: CrossingStateKind::OutOfReach,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredictorName {
    Oracle,
    CrowFlies,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassDetectorName {
    Offline,
    Online,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FloatTypeName {
    F32,
    F64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationRunRowV1 {
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub started_at: DateTime<Utc>,
    pub evaluation_version: String,
    pub code_commit: String,
    pub country: String,
    pub realtime_at_least: Percentage,
    pub sessions: u64,
    pub pick_seed: u64,
    pub predictor: PredictorName,
    pub predictor_horizon_ms: Option<i64>,
    pub predictor_radius_m: Option<f64>,
    pub pass_detector: PassDetectorName,
    pub float_type: FloatTypeName,
    pub horizon_ms: i64,
    pub last_moment_ms: i64,
    pub ramp_ms: i64,
    pub last_moment_score: f64,
    pub floor: f64,
    pub warm_up_ms: i64,
    pub resamples: u64,
    pub bootstrap_seed: u64,
    pub train_session_v3_fingerprint: String,
    pub train_session_sample_v3_fingerprint: String,
    pub train_session_crossing_v1_fingerprint: String,
    pub water_crossing_v2_fingerprint: String,
}

impl Row for EvaluationRunRowV1 {
    type Layer = layers::Gold;
    const DATASET: DatasetSpec<Self::Layer> = EVALUATION_RUN_V1;
    const INSTANTS: &'static [&'static str] = &["started_at"];

    fn example() -> Self {
        Self {
            started_at: DateTime::UNIX_EPOCH,
            evaluation_version: "replay-v1".to_string(),
            code_commit: "0000000000000000000000000000000000000000".to_string(),
            country: "DE".to_string(),
            realtime_at_least: Percentage::new(90.0).expect("90 is a percentage"),
            sessions: 1000,
            pick_seed: 1,
            predictor: PredictorName::Oracle,
            predictor_horizon_ms: Some(300_000),
            predictor_radius_m: Some(5_000.0),
            pass_detector: PassDetectorName::Offline,
            float_type: FloatTypeName::F32,
            horizon_ms: 300_000,
            last_moment_ms: 30_000,
            ramp_ms: 60_000,
            last_moment_score: 0.25,
            floor: 0.05,
            warm_up_ms: 60_000,
            resamples: 1000,
            bootstrap_seed: 1,
            train_session_v3_fingerprint: "example".to_string(),
            train_session_sample_v3_fingerprint: "example".to_string(),
            train_session_crossing_v1_fingerprint: "example".to_string(),
            water_crossing_v2_fingerprint: "example".to_string(),
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
