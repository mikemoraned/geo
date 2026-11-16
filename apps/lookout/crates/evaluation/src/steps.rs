use std::collections::BTreeMap;

use crossings::silver::{Crossing, ReadError, read_of_country};
use domain::{CoordinateError, Precision, SessionId};
use medallion::{Country, QueryError, Root};
use medallion_model::{
    EvaluationOutcomeRowV2, EvaluationSessionRowV2, EvaluationSkewRowV2, EvaluationSplitRowV1,
    EvaluationSummaryRowV2, EvaluationTransitionRowV1, FloatType, TrainSessionRowV3,
};
use predictor::{
    Alerting, AlertingBeyondWatching, Cone, CrowFlies, LastMove, LightCone, OnlineDetector,
    Watching,
};

use crate::a5::NoA5Cell;
use crate::check::{SplitBroken, SplitCheck, SplitCheckError, SplitCounts};
use crate::offline::{OfflineError, PassesBySession, passes_by_session};
use crate::oracle::{Oracle, OracleError};
use crate::pick::{Pick, PickError, pick};
use crate::record::{SessionRowsError, run_session_rows};
use crate::replay::{ReplayError, SessionReplay};
use crate::rows::{
    RowsError, outcome_rows, replayed_from, scored_from, skew_row, summary_row, transition_rows,
};
use crate::score::{Outcome, ScoreError, Scoring};
use crate::scored::ScoredCrossings;
use crate::settings::{
    CrowFliesSettings, LightConeSettings, OnlineDetectorSettings, OracleSettings,
};
use crate::skew::{SkewError, Visit, skew};
use crate::split::ScoredSplit;
use crate::store::{compact_ids, read_passes, read_samples};
use crate::summarise::{Bootstrap, SummariseError};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Predictor {
    Oracle(OracleSettings),
    CrowFlies(CrowFliesSettings),
    LightCone(LightConeSettings),
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationSettings {
    pub pick: Pick,
    pub predictor: Predictor,
    pub online_detector: Option<OnlineDetectorSettings>,
    pub float_type: FloatType,
    pub scoring: Scoring,
    pub bootstrap: Bootstrap,
    pub split: Option<ScoredSplit>,
}

impl EvaluationSettings {
    pub fn skew_settings(&self) -> Option<SkewSettings> {
        self.online_detector.map(|online_detector| SkewSettings {
            online_detector,
            bootstrap: self.bootstrap,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkewSettings {
    pub online_detector: OnlineDetectorSettings,
    pub bootstrap: Bootstrap,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReplayResults {
    pub transitions: Vec<EvaluationTransitionRowV1>,
    pub skew: Option<EvaluationSkewRowV2>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationResults {
    pub picked: Vec<TrainSessionRowV3>,
    pub transitions: Vec<EvaluationTransitionRowV1>,
    pub skew: Option<EvaluationSkewRowV2>,
    pub outcomes: Vec<EvaluationOutcomeRowV2>,
    pub split: Option<EvaluationSplitRowV1>,
    pub summary: EvaluationSummaryRowV2,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScoreResults {
    pub outcomes: Vec<EvaluationOutcomeRowV2>,
    pub split: Option<EvaluationSplitRowV1>,
}

#[derive(Debug, thiserror::Error)]
pub enum StepError {
    #[error(transparent)]
    Pick(#[from] PickError),
    #[error(transparent)]
    Sessions(#[from] SessionRowsError),
    #[error("placing a crossing in the split: {0}")]
    Split(#[from] NoA5Cell),
    #[error("checking the split: {0}")]
    SplitCheck(#[from] SplitCheckError),
    #[error("refusing the run: {0}")]
    SplitBroken(#[from] SplitBroken),
    #[error("reading the store: {0}")]
    Query(#[from] QueryError),
    #[error("reading the crossings: {0}")]
    Crossings(#[from] ReadError),
    #[error("gathering the sessions' passes: {0}")]
    Passes(#[from] OfflineError),
    #[error("a crossing lies at no position: {0}")]
    Position(#[from] CoordinateError),
    #[error(transparent)]
    Oracle(#[from] OracleError),
    #[error(transparent)]
    CrowFlies(#[from] AlertingBeyondWatching),
    #[error(transparent)]
    Replay(#[from] ReplayError),
    #[error(transparent)]
    Skew(#[from] SkewError),
    #[error(transparent)]
    Score(#[from] ScoreError),
    #[error(transparent)]
    Summarise(#[from] SummariseError),
    #[error(transparent)]
    Rows(#[from] RowsError),
}

#[derive(Debug, Clone)]
pub struct CrossingsAndPasses {
    pub crossings: Vec<Crossing>,
    pub passes: PassesBySession,
    pub scored: ScoredCrossings,
    pub checked: Option<CheckedSplit>,
}

#[derive(Debug, Clone)]
pub struct CheckedSplit {
    pub check: SplitCheck,
    pub counts: SplitCounts,
}

pub async fn read_crossings_and_passes(
    root: &Root,
    country: Country,
    sessions: &[EvaluationSessionRowV2],
    split: Option<&ScoredSplit>,
) -> Result<CrossingsAndPasses, StepError> {
    let check = match split {
        None => None,
        Some(split) => Some(SplitCheck::read(root, country, split).await?),
    };
    let crossings = match &check {
        None => read_of_country(root, country).await?,
        Some(check) => check.in_country(read_of_country(root, country).await?),
    };
    let passes = passes_by_session(
        sessions.iter().map(|session| session.session_id.clone()),
        read_passes(root, sessions).await?,
        &compact_ids(&crossings),
    )?;
    let scored = ScoredCrossings::of(&crossings, split)?;
    let checked = match check {
        None => None,
        Some(check) => {
            let samples = read_samples(
                root,
                country,
                sessions.iter().map(|session| &session.session_id),
            )
            .await?;
            let counts = check.before_replay(&crossings, &passes, &samples)?;
            Some(CheckedSplit { check, counts })
        }
    };
    Ok(CrossingsAndPasses {
        crossings,
        passes,
        scored,
        checked,
    })
}

pub async fn replay(
    root: &Root,
    country: Country,
    sessions: &[EvaluationSessionRowV2],
    predictor: Predictor,
    float_type: FloatType,
    skew_settings: Option<SkewSettings>,
    split: Option<&ScoredSplit>,
) -> Result<ReplayResults, StepError> {
    let crossings_and_passes = read_crossings_and_passes(root, country, sessions, split).await?;
    replay_against(
        root,
        country,
        sessions,
        &crossings_and_passes,
        predictor,
        float_type,
        skew_settings,
    )
    .await
}

pub async fn replay_against(
    root: &Root,
    country: Country,
    sessions: &[EvaluationSessionRowV2],
    crossings_and_passes: &CrossingsAndPasses,
    predictor: Predictor,
    float_type: FloatType,
    skew_settings: Option<SkewSettings>,
) -> Result<ReplayResults, StepError> {
    let online_detector = skew_settings.map(|skew_settings| skew_settings.online_detector);
    let replayed = match float_type {
        FloatType::F32 => {
            replayed::<f32>(
                root,
                country,
                sessions,
                crossings_and_passes,
                predictor,
                online_detector,
            )
            .await?
        }
        FloatType::F64 => {
            replayed::<f64>(
                root,
                country,
                sessions,
                crossings_and_passes,
                predictor,
                online_detector,
            )
            .await?
        }
    };
    let (transitions, visits): (BTreeMap<_, _>, BTreeMap<_, _>) = replayed
        .into_iter()
        .map(
            |(
                session_id,
                SessionReplay {
                    transitions,
                    visits,
                },
            )| { ((session_id.clone(), transitions), (session_id, visits)) },
        )
        .unzip();
    let scored = &crossings_and_passes.scored;
    let skew = skew_settings
        .map(|skew_settings| {
            let visits: BTreeMap<SessionId, Vec<Visit>> = visits
                .into_iter()
                .filter_map(|(session_id, visits)| {
                    Some((
                        session_id,
                        scored.keep(visits?, |visit| visit.crossing_compact_id),
                    ))
                })
                .collect();
            let passes: PassesBySession = crossings_and_passes
                .passes
                .iter()
                .map(|(session_id, passes)| {
                    (
                        session_id.clone(),
                        scored.keep(passes.iter().copied(), |&(_, crossing)| crossing),
                    )
                })
                .collect();
            skew(sessions, &passes, &visits, &skew_settings.bootstrap)
        })
        .transpose()?;
    Ok(ReplayResults {
        transitions: transition_rows(&transitions),
        skew: skew.as_ref().map(skew_row),
    })
}

async fn replayed<F: Precision + Send + Sync>(
    root: &Root,
    country: Country,
    sessions: &[EvaluationSessionRowV2],
    crossings_and_passes: &CrossingsAndPasses,
    predictor: Predictor,
    online_detector: Option<OnlineDetectorSettings>,
) -> Result<BTreeMap<SessionId, SessionReplay>, StepError> {
    let compact = crossings_and_passes
        .crossings
        .iter()
        .map(crossings::compacted::<F>)
        .collect::<Result<Vec<_>, _>>()?;
    let online_detector =
        online_detector.map(|settings| OnlineDetector::new(compact.clone(), settings.pass_radius));
    Ok(match predictor {
        Predictor::Oracle(OracleSettings { horizon }) => {
            let oracles = Oracle::per_session(&crossings_and_passes.passes, &compact, horizon)?;
            crate::replay::replay(
                root,
                country,
                sessions,
                &crossings_and_passes.passes,
                |session_id| {
                    // Infallible: `per_session` builds an oracle for every session it is given, and
                    // the replay asks for one per session of the same list.
                    oracles[session_id].clone()
                },
                online_detector.as_ref(),
            )
            .await?
        }
        Predictor::CrowFlies(CrowFliesSettings { radius, horizon }) => {
            let crow_flies =
                CrowFlies::new(compact, Watching { radius }, Alerting { radius, horizon })?;
            crate::replay::replay(
                root,
                country,
                sessions,
                &crossings_and_passes.passes,
                |_| crow_flies.clone(),
                online_detector.as_ref(),
            )
            .await?
        }
        Predictor::LightCone(LightConeSettings {
            radius,
            half_width,
            horizon,
            still_within,
        }) => {
            let light_cone = LightCone::new(
                compact,
                Watching { radius },
                Cone {
                    half_width,
                    horizon,
                },
                LastMove::new(still_within),
            );
            crate::replay::replay(
                root,
                country,
                sessions,
                &crossings_and_passes.passes,
                |_| light_cone.clone(),
                online_detector.as_ref(),
            )
            .await?
        }
    })
}

pub async fn score(
    root: &Root,
    country: Country,
    sessions: &[EvaluationSessionRowV2],
    transitions: &[EvaluationTransitionRowV1],
    scoring: &Scoring,
    split: Option<&ScoredSplit>,
) -> Result<ScoreResults, StepError> {
    let crossings_and_passes = read_crossings_and_passes(root, country, sessions, split).await?;
    score_against(sessions, &crossings_and_passes, transitions, scoring)
}

pub fn score_against(
    sessions: &[EvaluationSessionRowV2],
    crossings_and_passes: &CrossingsAndPasses,
    transitions: &[EvaluationTransitionRowV1],
    scoring: &Scoring,
) -> Result<ScoreResults, StepError> {
    let replayed = replayed_from(sessions, transitions)?;
    let every = crate::score::score(sessions, &replayed, &crossings_and_passes.passes, scoring)?;
    let split = crossings_and_passes
        .checked
        .as_ref()
        .map(|checked| {
            checked
                .check
                .of_outcomes(&crossings_and_passes.crossings, &every)
                .map(|outcomes| checked.counts.row(outcomes))
        })
        .transpose()?;
    let scored = every
        .into_iter()
        .map(|(session_id, outcomes)| {
            (
                session_id,
                crossings_and_passes
                    .scored
                    .keep(outcomes, Outcome::crossing_compact_id),
            )
        })
        .collect();
    Ok(ScoreResults {
        outcomes: outcome_rows(&scored),
        split,
    })
}

pub fn summarise(
    sessions: &[EvaluationSessionRowV2],
    outcomes: &[EvaluationOutcomeRowV2],
    bootstrap: &Bootstrap,
) -> Result<EvaluationSummaryRowV2, StepError> {
    let scored = scored_from(sessions, outcomes)?;
    Ok(summary_row(&crate::summarise::summarise(
        sessions, &scored, bootstrap,
    )?))
}

pub async fn run(root: &Root, run: &EvaluationSettings) -> Result<EvaluationResults, StepError> {
    let country = run.pick.country;
    let picked = pick(root, &run.pick).await?;
    let sessions = run_session_rows(root, run.pick.country, run.split.as_ref(), &picked).await?;
    let crossings_and_passes =
        read_crossings_and_passes(root, country, &sessions, run.split.as_ref()).await?;
    let ReplayResults { transitions, skew } = replay_against(
        root,
        country,
        &sessions,
        &crossings_and_passes,
        run.predictor,
        run.float_type,
        run.skew_settings(),
    )
    .await?;
    let ScoreResults { outcomes, split } =
        score_against(&sessions, &crossings_and_passes, &transitions, &run.scoring)?;
    let summary = summarise(&sessions, &outcomes, &run.bootstrap)?;
    Ok(EvaluationResults {
        picked,
        transitions,
        skew,
        outcomes,
        split,
        summary,
    })
}
#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use domain::{HalfWidth, Radius, Span};

    use super::*;
    use crate::fixtures::{around_dresden_hbf, oracle, run_of, store, write_country};
    use crate::record::session_rows;
    use crate::split::{Split, Splitter};

    #[tokio::test]
    async fn a_whole_oracle_run_scores_perfectly() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = store(tmp.path(), 3).await;

        let results = run(&root, &run_of(oracle())).await.expect("a run");

        assert_eq!(results.picked.len(), 3);
        assert_eq!(
            (
                results.summary.sessions,
                results.summary.passes,
                results.summary.notifications,
                results.summary.false_alarms,
                results.summary.misses
            ),
            (3, 3, 3, 0, 0)
        );
        assert_eq!(
            (
                results.summary.precision,
                results.summary.precision_low,
                results.summary.recall,
                results.summary.recall_low
            ),
            (Some(1.0), Some(1.0), Some(1.0), Some(1.0))
        );
    }

    #[tokio::test]
    async fn a_whole_crow_flies_run_notifies_nothing_and_misses_every_pass() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = store(tmp.path(), 3).await;

        let results = run(
            &root,
            &run_of(Predictor::CrowFlies(CrowFliesSettings {
                radius: Radius::new(5_000.0).expect("a radius"),
                horizon: Span::new("5m").expect("a span"),
            })),
        )
        .await
        .expect("a run");

        assert!(!results.transitions.is_empty());
        assert_eq!(
            (
                results.summary.notifications,
                results.summary.misses,
                results.summary.recall
            ),
            (0, 3, Some(0.0))
        );
    }

    #[tokio::test]
    async fn a_whole_light_cone_run_notifies_nothing_and_misses_every_pass() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = store(tmp.path(), 3).await;

        let results = run(
            &root,
            &run_of(Predictor::LightCone(LightConeSettings {
                radius: Radius::new(20_000.0).expect("a radius"),
                half_width: HalfWidth::new(45.0).expect("a half-width"),
                horizon: Span::new("5m").expect("a span"),
                still_within: Radius::new(20.0).expect("a radius"),
            })),
        )
        .await
        .expect("a run");

        assert!(!results.transitions.is_empty());
        assert_eq!(
            (
                results.summary.notifications,
                results.summary.misses,
                results.summary.recall
            ),
            (0, 3, Some(0.0))
        );
    }

    #[tokio::test]
    async fn the_tune_and_test_runs_sum_to_the_run_without_a_split() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = store(tmp.path(), 3).await;
        write_country(&root, around_dresden_hbf(1.0)).await;
        let whole = EvaluationSettings {
            online_detector: Some(OnlineDetectorSettings {
                pass_radius: Radius::new(100.0).expect("a radius"),
            }),
            ..run_of(oracle())
        };
        let scoring = |split| EvaluationSettings {
            split: Some(ScoredSplit {
                splitter: Splitter {
                    resolution: "8".parse().expect("a resolution"),
                    salt: "tune-test-1".parse().expect("a salt"),
                    tune_buckets: "70".parse().expect("tune buckets"),
                },
                split,
            }),
            ..whole.clone()
        };
        let counts = |results: &EvaluationResults| {
            let (summary, skew) = (&results.summary, results.skew.as_ref().expect("a skew"));
            [
                summary.passes,
                summary.notifications,
                summary.false_alarms,
                summary.repeats,
                summary.misses,
                skew.passes,
                skew.visits,
            ]
        };

        let all = run(&root, &whole).await.expect("a run");
        let tune = run(&root, &scoring(Split::Tune)).await.expect("a run");
        let test = run(&root, &scoring(Split::Test)).await.expect("a run");

        let summed: Vec<u64> = counts(&tune)
            .iter()
            .zip(counts(&test))
            .map(|(tune, test)| tune + test)
            .collect();
        assert_eq!(summed, counts(&all));
        assert_eq!(
            (
                all.summary.scored_duration,
                tune.summary.scored_duration,
                test.summary.scored_duration
            ),
            (
                TimeDelta::minutes(30),
                TimeDelta::zero(),
                TimeDelta::minutes(30)
            )
        );
        assert_eq!(
            (all.split, tune.split, test.split),
            (
                None,
                Some(EvaluationSplitRowV1 {
                    cells: 2,
                    cells_in_split: 1,
                    crossings: 2,
                    crossings_in_split: 1,
                    passes_in_split: 0,
                    passes_outside: 3,
                    outcomes_in_split: 0,
                    outcomes_outside: 3,
                    cells_outside_cover: 0,
                }),
                Some(EvaluationSplitRowV1 {
                    cells: 2,
                    cells_in_split: 1,
                    crossings: 2,
                    crossings_in_split: 1,
                    passes_in_split: 3,
                    passes_outside: 0,
                    outcomes_in_split: 3,
                    outcomes_outside: 0,
                    cells_outside_cover: 0,
                })
            )
        );
    }

    #[tokio::test]
    async fn a_run_with_a_split_leaves_out_the_crossings_outside_the_country() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = store(tmp.path(), 3).await;
        write_country(&root, around_dresden_hbf(0.1)).await;
        let settings = EvaluationSettings {
            split: Some(ScoredSplit {
                splitter: Splitter {
                    resolution: "8".parse().expect("a resolution"),
                    salt: "tune-test-1".parse().expect("a salt"),
                    tune_buckets: "70".parse().expect("tune buckets"),
                },
                split: Split::Test,
            }),
            ..run_of(oracle())
        };

        let results = run(&root, &settings).await.expect("a run");

        assert_eq!(
            results.split,
            Some(EvaluationSplitRowV1 {
                cells: 1,
                cells_in_split: 1,
                crossings: 1,
                crossings_in_split: 1,
                passes_in_split: 3,
                passes_outside: 0,
                outcomes_in_split: 3,
                outcomes_outside: 0,
                cells_outside_cover: 0,
            })
        );
    }

    #[tokio::test]
    async fn a_run_with_a_split_over_a_store_holding_no_country_is_refused() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = store(tmp.path(), 3).await;
        let settings = EvaluationSettings {
            split: Some(ScoredSplit {
                splitter: Splitter {
                    resolution: "8".parse().expect("a resolution"),
                    salt: "tune-test-1".parse().expect("a salt"),
                    tune_buckets: "70".parse().expect("tune buckets"),
                },
                split: Split::Test,
            }),
            ..run_of(oracle())
        };

        let refused = run(&root, &settings).await;

        assert!(
            matches!(refused, Err(StepError::SplitCheck(_))),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn the_steps_run_one_by_one_give_what_the_run_gives() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let root = store(tmp.path(), 3).await;
        let settings = EvaluationSettings {
            online_detector: Some(OnlineDetectorSettings {
                pass_radius: Radius::new(100.0).expect("a radius"),
            }),
            ..run_of(oracle())
        };
        let results = run(&root, &settings).await.expect("a run");
        let sessions = session_rows(&results.picked);

        let ReplayResults { transitions, skew } = replay(
            &root,
            Country::Germany,
            &sessions,
            settings.predictor,
            settings.float_type,
            settings.skew_settings(),
            None,
        )
        .await
        .expect("a replay");
        let ScoreResults { outcomes, .. } = score(
            &root,
            Country::Germany,
            &sessions,
            &transitions,
            &settings.scoring,
            None,
        )
        .await
        .expect("scores");
        let summary = summarise(&sessions, &outcomes, &settings.bootstrap).expect("a summary");

        assert!(
            results.skew.is_some(),
            "a run with an online detector measures the skew"
        );
        assert_eq!(
            (transitions, skew, outcomes, summary),
            (
                results.transitions,
                results.skew,
                results.outcomes,
                results.summary
            )
        );
    }
}
