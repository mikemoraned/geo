use std::num::NonZeroUsize;

use chrono::{DateTime, NaiveDate, TimeDelta, Utc};
use code_version::CodeVersion;
use domain::{SessionId, Span};
use medallion::{FingerprintError, QueryError, Root, RunId};
use medallion_model::{EvaluationRunRowV4, EvaluationSessionRowV2, TrainSessionRowV3};
use uuid::Uuid;

use crate::a5::NoA5Cell;
use crate::fingerprints::Fingerprints;
use crate::pick::{Pick, Seed};
use crate::score::{Score, Scoring};
use crate::scored::scored_hours;
use crate::settings::{SettingsError, read_online_detector, write_online_detector};
use crate::split::ScoredSplit;
use crate::steps::{EvaluationSettings, Predictor};
use crate::store::read_samples;
use crate::summarise::Bootstrap;

pub const EVALUATION_VERSION: &str = "replay-v1";

const RUN_NAMESPACE: &[u8] = b"EVALUATION_RUN";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inputs {
    pub code_version: String,
    pub fingerprints: Fingerprints,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordResults {
    pub run_id: RunId,
    pub date: NaiveDate,
    pub row: EvaluationRunRowV4,
    pub sessions: Vec<EvaluationSessionRowV2>,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionRowsError {
    #[error("reading the sessions' samples: {0}")]
    Query(#[from] QueryError),
    #[error("the session {session_id} was picked, and the store holds no samples for it")]
    NoSamples { session_id: SessionId },
    #[error("placing a sample in the split: {0}")]
    Split(#[from] NoA5Cell),
}

#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    #[error(transparent)]
    Fingerprint(#[from] FingerprintError),
    #[error(transparent)]
    Sessions(#[from] SessionRowsError),
    #[error("the run's predictor settings cannot be read back: {0}")]
    PredictorSettings(SettingsError),
    #[error("the run's online detector settings cannot be read back: {0}")]
    OnlineDetectorSettings(SettingsError),
    #[error("the run's `{column}` cannot be read back as a setting: {reason}")]
    Setting {
        column: &'static str,
        reason: String,
    },
}

fn namespace(name: &[u8]) -> Uuid {
    Uuid::new_v5(&Uuid::nil(), name)
}

pub async fn record(
    root: &Root,
    run: &EvaluationSettings,
    code: &CodeVersion,
    picked: &[TrainSessionRowV3],
) -> Result<RecordResults, RecordError> {
    let inputs = Inputs {
        code_version: code.as_str().to_string(),
        fingerprints: Fingerprints::read(root).await?,
    };
    let row = run_row(run, &inputs, Utc::now());
    Ok(RecordResults {
        run_id: run_id(&row),
        date: row.started_at.date_naive(),
        row,
        sessions: run_session_rows(root, run, picked).await?,
    })
}

pub fn run_row(
    run: &EvaluationSettings,
    inputs: &Inputs,
    started_at: DateTime<Utc>,
) -> EvaluationRunRowV4 {
    EvaluationRunRowV4 {
        started_at,
        evaluation_version: EVALUATION_VERSION.to_string(),
        code_version: inputs.code_version.clone(),
        country: run.pick.country.to_string(),
        realtime_at_least: run.pick.realtime_at_least,
        sessions: run.pick.sessions.get() as u64,
        pick_seed: run.pick.seed.0,
        predictor: run.predictor.name(),
        predictor_settings: run.predictor.settings(),
        online_detector_settings: write_online_detector(run.online_detector),
        float_type: run.float_type,
        horizon_ms: milliseconds(run.scoring.horizon),
        last_moment_ms: milliseconds(run.scoring.last_moment),
        ramp_ms: milliseconds(run.scoring.ramp),
        last_moment_score: run.scoring.last_moment_score.get(),
        floor: run.scoring.floor.get(),
        warm_up_ms: milliseconds(run.scoring.warm_up),
        resamples: run.bootstrap.resamples.get() as u64,
        bootstrap_seed: run.bootstrap.seed.0,
        fingerprints: inputs.fingerprints.clone().into(),
    }
}

pub fn run_id(row: &EvaluationRunRowV4) -> RunId {
    let unstarted = EvaluationRunRowV4 {
        started_at: DateTime::UNIX_EPOCH,
        ..row.clone()
    };
    // Infallible: a run row's one map is keyed by text, and serde_json refuses only other keys.
    let canonical = serde_json::to_vec(&unstarted).expect("a run row writes as JSON");
    let id = Uuid::new_v5(&namespace(RUN_NAMESPACE), &canonical);
    // Infallible: a UUID's text holds none of the characters a partition value refuses.
    RunId::new(id.to_string()).expect("a UUID names a partition")
}

impl TryFrom<&EvaluationRunRowV4> for EvaluationSettings {
    type Error = RecordError;

    fn try_from(row: &EvaluationRunRowV4) -> Result<Self, Self::Error> {
        Ok(EvaluationSettings {
            pick: Pick {
                country: row.country.parse().map_err(|err| setting("country", err))?,
                realtime_at_least: row.realtime_at_least,
                sessions: count("sessions", row.sessions)?,
                seed: Seed(row.pick_seed),
            },
            predictor: Predictor::read(row.predictor, &row.predictor_settings)
                .map_err(RecordError::PredictorSettings)?,
            online_detector: read_online_detector(&row.online_detector_settings)
                .map_err(RecordError::OnlineDetectorSettings)?,
            float_type: row.float_type,
            scoring: Scoring {
                horizon: span_of("horizon_ms", row.horizon_ms)?,
                last_moment: span_of("last_moment_ms", row.last_moment_ms)?,
                ramp: span_of("ramp_ms", row.ramp_ms)?,
                last_moment_score: Score::new(row.last_moment_score)
                    .map_err(|err| setting("last_moment_score", err))?,
                floor: Score::new(row.floor).map_err(|err| setting("floor", err))?,
                warm_up: span_of("warm_up_ms", row.warm_up_ms)?,
            },
            bootstrap: Bootstrap {
                resamples: count("resamples", row.resamples)?,
                seed: Seed(row.bootstrap_seed),
            },
            split: None,
        })
    }
}

pub fn session_rows(picked: &[TrainSessionRowV3]) -> Vec<EvaluationSessionRowV2> {
    picked
        .iter()
        .map(|session| {
            let hours = hours(session);
            session_row(session, hours, hours)
        })
        .collect()
}

pub async fn run_session_rows(
    root: &Root,
    run: &EvaluationSettings,
    picked: &[TrainSessionRowV3],
) -> Result<Vec<EvaluationSessionRowV2>, SessionRowsError> {
    match &run.split {
        None => Ok(session_rows(picked)),
        Some(split) => split_session_rows(root, run, picked, split).await,
    }
}

async fn split_session_rows(
    root: &Root,
    run: &EvaluationSettings,
    picked: &[TrainSessionRowV3],
    split: &ScoredSplit,
) -> Result<Vec<EvaluationSessionRowV2>, SessionRowsError> {
    let samples = read_samples(
        root,
        run.pick.country,
        picked.iter().map(|session| &session.session_id),
    )
    .await?;
    picked
        .iter()
        .map(|session| {
            let samples =
                samples
                    .get(&session.session_id)
                    .ok_or_else(|| SessionRowsError::NoSamples {
                        session_id: session.session_id.clone(),
                    })?;
            Ok(session_row(
                session,
                hours(session),
                scored_hours(samples, split)?,
            ))
        })
        .collect()
}

fn hours(session: &TrainSessionRowV3) -> f64 {
    (session.ended_at - session.started_at).as_seconds_f64() / 3_600.0
}

fn session_row(
    session: &TrainSessionRowV3,
    hours: f64,
    scored_hours: f64,
) -> EvaluationSessionRowV2 {
    EvaluationSessionRowV2 {
        session_id: session.session_id.clone(),
        started_at: session.started_at,
        ended_at: session.ended_at,
        hours,
        scored_hours,
    }
}

fn milliseconds(span: Span) -> i64 {
    span.time_delta().num_milliseconds()
}

fn span_of(column: &'static str, milliseconds: i64) -> Result<Span, RecordError> {
    Span::try_from(TimeDelta::milliseconds(milliseconds)).map_err(|err| setting(column, err))
}

fn count(column: &'static str, value: u64) -> Result<NonZeroUsize, RecordError> {
    usize::try_from(value)
        .ok()
        .and_then(NonZeroUsize::new)
        .ok_or_else(|| setting(column, format!("{value} is not a count of at least 1")))
}

fn setting(column: &'static str, reason: impl std::fmt::Display) -> RecordError {
    RecordError::Setting {
        column,
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use medallion_model::FloatType;
    use std::collections::BTreeMap;
    use std::num::NonZeroUsize;

    use domain::{Percentage, Radius};
    use medallion::Country;

    use super::*;
    use crate::fixtures::instant;
    use crate::settings::{CrowFliesSettings, OnlineDetectorSettings, OracleSettings};

    fn span(text: &str) -> Span {
        Span::new(text).expect("a span")
    }

    fn oracle_run() -> EvaluationSettings {
        EvaluationSettings {
            pick: Pick {
                country: Country::Germany,
                realtime_at_least: Percentage::new(90.0).expect("a percentage"),
                sessions: NonZeroUsize::new(1000).expect("not zero"),
                seed: Seed(1),
            },
            predictor: Predictor::Oracle(OracleSettings {
                horizon: span("5m"),
            }),
            online_detector: None,
            float_type: FloatType::F32,
            scoring: Scoring {
                horizon: span("5m"),
                last_moment: span("30s"),
                ramp: span("1m"),
                last_moment_score: Score::new(0.25).expect("a score"),
                floor: Score::new(0.05).expect("a score"),
                warm_up: span("1m"),
            },
            bootstrap: Bootstrap {
                resamples: NonZeroUsize::new(1000).expect("not zero"),
                seed: Seed(1),
            },
            split: None,
        }
    }

    fn crow_flies_run() -> EvaluationSettings {
        EvaluationSettings {
            predictor: Predictor::CrowFlies(CrowFliesSettings {
                radius: Radius::new(5_000.0).expect("a radius"),
                horizon: span("5m"),
            }),
            online_detector: Some(OnlineDetectorSettings {
                pass_radius: Radius::new(100.0).expect("a radius"),
            }),
            float_type: FloatType::F64,
            ..oracle_run()
        }
    }

    fn inputs() -> Inputs {
        Inputs {
            code_version: "abc1234".to_string(),
            fingerprints: Fingerprints::from(BTreeMap::from([
                ("train_session_v3".to_string(), "f1".to_string()),
                ("water_crossing_v2".to_string(), "f4".to_string()),
            ])),
        }
    }

    #[test]
    fn a_runs_settings_survive_a_round_trip_through_its_row() {
        for run in [oracle_run(), crow_flies_run()] {
            let row = run_row(&run, &inputs(), instant(0));

            assert_eq!(
                EvaluationSettings::try_from(&row).expect("the run's settings"),
                run
            );
        }
    }

    #[test]
    fn a_run_row_records_its_inputs_and_the_evaluations_version() {
        let row = run_row(&oracle_run(), &inputs(), instant(0));

        assert_eq!(
            (
                row.evaluation_version.as_str(),
                row.code_version.as_str(),
                row.fingerprints["water_crossing_v2"].as_str(),
                row.started_at
            ),
            (EVALUATION_VERSION, "abc1234", "f4", instant(0))
        );
    }

    #[test]
    fn a_run_started_later_with_the_same_settings_and_inputs_has_the_same_id() {
        let now = run_row(&oracle_run(), &inputs(), instant(0));
        let later = run_row(&oracle_run(), &inputs(), instant(86_400));

        assert_eq!(run_id(&now), run_id(&later));
    }

    #[test]
    fn a_change_of_setting_code_or_data_changes_the_id() {
        let id = run_id(&run_row(&oracle_run(), &inputs(), instant(0)));
        let recoded = Inputs {
            code_version: "def5678".to_string(),
            ..inputs()
        };
        let rederived = Inputs {
            fingerprints: Fingerprints::from(BTreeMap::from([
                ("train_session_v3".to_string(), "changed".to_string()),
                ("water_crossing_v2".to_string(), "f4".to_string()),
            ])),
            ..inputs()
        };

        for other in [
            run_row(&crow_flies_run(), &inputs(), instant(0)),
            run_row(&oracle_run(), &recoded, instant(0)),
            run_row(&oracle_run(), &rederived, instant(0)),
        ] {
            assert_ne!(run_id(&other), id);
        }
    }

    #[test]
    fn a_run_id_is_a_uuid() {
        let id = run_id(&run_row(&oracle_run(), &inputs(), instant(0)));

        assert!(uuid::Uuid::parse_str(&id.to_string()).is_ok(), "{id}");
    }

    #[test]
    fn an_oracle_row_without_its_horizon_is_refused() {
        let row = EvaluationRunRowV4 {
            predictor_settings: BTreeMap::new(),
            ..run_row(&oracle_run(), &inputs(), instant(0))
        };

        assert!(
            matches!(
                EvaluationSettings::try_from(&row),
                Err(RecordError::PredictorSettings(SettingsError::Missing(
                    "horizon"
                )))
            ),
            "{:?}",
            EvaluationSettings::try_from(&row)
        );
    }

    #[tokio::test]
    async fn a_recorded_run_names_its_code_inputs_and_sessions_under_its_id() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = crate::fixtures::store(tmp.path(), 3).await;
        let run = crate::fixtures::run_of(crate::fixtures::oracle());
        let picked = crate::pick::pick(&root, &run.pick).await.expect("a pick");
        let code = CodeVersion::new("abc1234").expect("a code version");

        let recorded = record(&root, &run, &code, &picked)
            .await
            .expect("a recorded run");

        let inputs = Inputs {
            code_version: "abc1234".to_string(),
            fingerprints: Fingerprints::read(&root).await.expect("fingerprints"),
        };
        assert_eq!(
            recorded.row,
            run_row(&run, &inputs, recorded.row.started_at)
        );
        assert_eq!(recorded.run_id, run_id(&recorded.row));
        assert_eq!(recorded.date, recorded.row.started_at.date_naive());
        assert_eq!(recorded.sessions, session_rows(&picked));
    }
}
