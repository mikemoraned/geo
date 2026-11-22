use std::num::NonZeroUsize;

use chrono::{DateTime, NaiveDate, TimeDelta, Utc};
use code_version::CodeVersion;
use domain::{SessionId, Span};
use itertools::Itertools;
use medallion::{Country, FingerprintError, QueryError, Root, RunId};
use medallion_model::{
    EvaluationRunRowV6, EvaluationSessionRowV3, TrainSessionCrossingRowV1, TrainSessionRowV3,
};
use uuid::Uuid;

use crate::a5::NoA5Cell;
use crate::fingerprints::Fingerprints;
use crate::pick::{Pick, Seed};
use crate::score::{Score, Scoring};
use crate::scored::scored_duration;
use crate::settings::{
    SettingsError, read_online_detector, read_split, write_online_detector, write_split,
};
use crate::split::ScoredSplit;
use crate::steps::{EvaluationSettings, Predictor};
use crate::store::{read_passes, read_samples};
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
    pub row: EvaluationRunRowV6,
    pub sessions: Vec<EvaluationSessionRowV3>,
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
    #[error("the run's split settings cannot be read back: {0}")]
    SplitSettings(SettingsError),
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
        sessions: run_session_rows(root, run.pick.country, run.split.as_ref(), picked).await?,
    })
}

pub fn run_row(
    run: &EvaluationSettings,
    inputs: &Inputs,
    started_at: DateTime<Utc>,
) -> EvaluationRunRowV6 {
    EvaluationRunRowV6 {
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
        split_settings: write_split(run.split.as_ref()),
        float_type: run.float_type,
        horizon: run.scoring.horizon.time_delta(),
        last_moment: run.scoring.last_moment.time_delta(),
        ramp: run.scoring.ramp.time_delta(),
        last_moment_score: run.scoring.last_moment_score.get(),
        floor: run.scoring.floor.get(),
        warm_up: run.scoring.warm_up.time_delta(),
        resamples: run.bootstrap.resamples.get() as u64,
        bootstrap_seed: run.bootstrap.seed.0,
        fingerprints: inputs.fingerprints.clone().into(),
    }
}

pub fn run_id(row: &EvaluationRunRowV6) -> RunId {
    let unstarted = EvaluationRunRowV6 {
        started_at: DateTime::UNIX_EPOCH,
        ..row.clone()
    };
    #[expect(
        clippy::unwrap_used,
        reason = "serde_json fails only on a map whose keys are not text, and the run row's only map has text keys"
    )]
    let canonical = serde_json::to_vec(&unstarted).unwrap();
    let id = Uuid::new_v5(&namespace(RUN_NAMESPACE), &canonical);
    RunId::from(id)
}

impl TryFrom<&EvaluationRunRowV6> for EvaluationSettings {
    type Error = RecordError;

    fn try_from(row: &EvaluationRunRowV6) -> Result<Self, Self::Error> {
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
                horizon: span_of("horizon", row.horizon)?,
                last_moment: span_of("last_moment", row.last_moment)?,
                ramp: span_of("ramp", row.ramp)?,
                last_moment_score: Score::new(row.last_moment_score)
                    .map_err(|err| setting("last_moment_score", err))?,
                floor: Score::new(row.floor).map_err(|err| setting("floor", err))?,
                warm_up: span_of("warm_up", row.warm_up)?,
            },
            bootstrap: Bootstrap {
                resamples: count("resamples", row.resamples)?,
                seed: Seed(row.bootstrap_seed),
            },
            split: read_split(&row.split_settings).map_err(RecordError::SplitSettings)?,
        })
    }
}

pub fn session_rows(
    picked: &[TrainSessionRowV3],
    passes: &[TrainSessionCrossingRowV1],
) -> Vec<EvaluationSessionRowV3> {
    in_route_order(picked, |session| &session.session_id, passes)
        .map(|(index, session)| {
            let duration = duration(session);
            session_row(index, session, duration, duration)
        })
        .collect()
}

pub fn in_route_order<'a, T>(
    sessions: &'a [T],
    session_id: impl Fn(&T) -> &SessionId,
    passes: &[TrainSessionCrossingRowV1],
) -> impl Iterator<Item = (u32, &'a T)> {
    let routes = passes
        .iter()
        .sorted_by_key(|pass| pass.crossed_at)
        .map(|pass| (&pass.session_id, &pass.crossing_id))
        .into_group_map();
    let no_route = Vec::new();
    sessions
        .iter()
        .sorted_by_cached_key(|session| {
            let id = session_id(session);
            (routes.get(id).unwrap_or(&no_route).clone(), id.clone())
        })
        .zip(0..)
        .map(|(session, index)| (index, session))
}

pub async fn run_session_rows(
    root: &Root,
    country: Country,
    split: Option<&ScoredSplit>,
    picked: &[TrainSessionRowV3],
) -> Result<Vec<EvaluationSessionRowV3>, SessionRowsError> {
    let passes = read_passes(root, picked.iter().map(|session| &session.session_id)).await?;
    match split {
        None => Ok(session_rows(picked, &passes)),
        Some(split) => split_session_rows(root, country, picked, &passes, split).await,
    }
}

async fn split_session_rows(
    root: &Root,
    country: Country,
    picked: &[TrainSessionRowV3],
    passes: &[TrainSessionCrossingRowV1],
    split: &ScoredSplit,
) -> Result<Vec<EvaluationSessionRowV3>, SessionRowsError> {
    let samples = read_samples(
        root,
        country,
        picked.iter().map(|session| &session.session_id),
    )
    .await?;
    in_route_order(picked, |session| &session.session_id, passes)
        .map(|(index, session)| {
            let samples =
                samples
                    .get(&session.session_id)
                    .ok_or_else(|| SessionRowsError::NoSamples {
                        session_id: session.session_id.clone(),
                    })?;
            Ok(session_row(
                index,
                session,
                duration(session),
                scored_duration(samples, split)?,
            ))
        })
        .collect()
}

fn duration(session: &TrainSessionRowV3) -> TimeDelta {
    session.ended_at - session.started_at
}

fn session_row(
    session_index: u32,
    session: &TrainSessionRowV3,
    duration: TimeDelta,
    scored_duration: TimeDelta,
) -> EvaluationSessionRowV3 {
    EvaluationSessionRowV3 {
        session_index,
        session_id: session.session_id.clone(),
        started_at: session.started_at,
        ended_at: session.ended_at,
        duration,
        scored_duration,
    }
}

fn span_of(column: &'static str, duration: TimeDelta) -> Result<Span, RecordError> {
    Span::try_from(duration).map_err(|err| setting(column, err))
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
    use crate::a5::A5Resolution;
    use crate::fixtures::instant;
    use crate::settings::{CrowFliesSettings, OnlineDetectorSettings, OracleSettings};
    use crate::split::{Salt, Split, Splitter, TuneBuckets};

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
                still_within: Radius::new(20.0).expect("a radius"),
            }),
            online_detector: Some(OnlineDetectorSettings {
                pass_radius: Radius::new(100.0).expect("a radius"),
            }),
            float_type: FloatType::F64,
            ..oracle_run()
        }
    }

    fn split_run() -> EvaluationSettings {
        EvaluationSettings {
            split: Some(ScoredSplit {
                splitter: Splitter {
                    resolution: A5Resolution::new(8).expect("a resolution"),
                    salt: Salt::new("tune-test-1").expect("a salt"),
                    tune_buckets: TuneBuckets::new(70).expect("tune buckets"),
                },
                split: Split::Test,
            }),
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
        for run in [oracle_run(), crow_flies_run(), split_run()] {
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
            run_row(&split_run(), &inputs(), instant(0)),
            run_row(&oracle_run(), &recoded, instant(0)),
            run_row(&oracle_run(), &rederived, instant(0)),
        ] {
            assert_ne!(run_id(&other), id);
        }
    }

    #[test]
    fn a_run_row_records_the_split_it_scores_and_its_splitter() {
        let row = run_row(&split_run(), &inputs(), instant(0));

        assert_eq!(
            row.split_settings,
            BTreeMap::from([
                ("resolution".to_string(), "8".to_string()),
                ("salt".to_string(), "tune-test-1".to_string()),
                ("split".to_string(), "test".to_string()),
                ("tune_buckets".to_string(), "70".to_string()),
            ])
        );
    }

    #[test]
    fn a_run_without_a_split_records_no_split_settings() {
        let row = run_row(&oracle_run(), &inputs(), instant(0));

        assert!(row.split_settings.is_empty(), "{:?}", row.split_settings);
    }

    #[test]
    fn a_run_id_is_a_uuid() {
        let id = run_id(&run_row(&oracle_run(), &inputs(), instant(0)));

        assert!(uuid::Uuid::parse_str(&id.to_string()).is_ok(), "{id}");
    }

    #[test]
    fn an_oracle_row_without_its_horizon_is_refused() {
        let row = EvaluationRunRowV6 {
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

    #[test]
    fn sessions_are_numbered_in_the_order_of_the_crossings_they_pass() {
        let session = |name: &str| TrainSessionRowV3 {
            session_id: SessionId::new(format!("session-{name}")).expect("a session id"),
            ..medallion::Row::example()
        };
        let pass = |name: &str, crossing: &str, seconds: i64| TrainSessionCrossingRowV1 {
            session_id: SessionId::new(format!("session-{name}")).expect("a session id"),
            crossing_id: domain::CrossingId::new(crossing).expect("a crossing id"),
            crossed_at: DateTime::UNIX_EPOCH + TimeDelta::seconds(seconds),
            ..medallion::Row::example()
        };
        let picked = [session("a"), session("b"), session("c"), session("d")];
        let passes = [
            pass("a", "y", 20),
            pass("a", "x", 10),
            pass("b", "w", 10),
            pass("c", "x", 10),
            pass("c", "y", 20),
        ];

        let numbered: Vec<(String, u32)> = session_rows(&picked, &passes)
            .into_iter()
            .map(|row| (row.session_id.to_string(), row.session_index))
            .sorted()
            .collect();

        assert_eq!(
            numbered,
            [
                ("session-a".to_string(), 2),
                ("session-b".to_string(), 1),
                ("session-c".to_string(), 3),
                ("session-d".to_string(), 0),
            ]
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
        let passes = read_passes(&root, picked.iter().map(|session| &session.session_id))
            .await
            .expect("the passes");
        assert_eq!(recorded.sessions, session_rows(&picked, &passes));
    }
}
