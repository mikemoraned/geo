use chrono::{DateTime, Utc};
use domain::{CrossingCompactId, SessionId};
use medallion::{DatasetSpec, RUN_DATE, Row, layers};
use serde::{Deserialize, Serialize};

use crate::TrainSessionRowV3;

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
