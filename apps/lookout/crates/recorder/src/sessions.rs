//! Splitting the bronze GPS samples into sessions: one contiguous run of samples from one
//! device.
//!
//! A run derives the boundaries from all of bronze rather than from what has arrived since
//! the last one. The newest session is always still open — more of its samples arrive with
//! the next drain — so a run has to be able to re-derive a session it has already written
//! and reach the same answer.
//!
//! Bronze tolerates the same observation arriving twice, so the samples are deduped on
//! `(device_id, t)` before anything looks at the intervals between them: a repeated sample
//! left in place is a zero-length interval, which is not a silence and must not be read as
//! one.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use geo_types::Point;
use medallion::{Query, Root};
use model::{DeviceId, SessionId, StartedBy};
use serde::{Deserialize, Serialize};

/// The deduped samples under their query name.
const SAMPLES: &str = "samples";

/// The session starts under their query name.
const SESSION_STARTS: &str = "session_starts";

/// One row per distinct sample, ordered so that each device's samples are one adjacent
/// run in time order.
///
/// Duplicates are collapsed by keeping the first row of a total order over the reported
/// values, rather than an arbitrary one: two rows sharing an identity but disagreeing on
/// what was measured would otherwise let a rerun pick differently and derive different
/// sessions from the same bronze.
const DISTINCT_SAMPLES: &str = "
    SELECT device_id, t, lat, lon, alt, acc, speed, heading
    FROM (
      SELECT *, ROW_NUMBER() OVER (
        PARTITION BY device_id, t ORDER BY lat, lon, alt, acc, speed, heading
      ) AS rank
      FROM samples
    )
    WHERE rank = 1
    ORDER BY device_id, t
";

/// One row per distinct session start, in the same order.
const DISTINCT_SESSION_STARTS: &str = "
    SELECT DISTINCT device_id, t FROM session_starts ORDER BY device_id, t
";

/// A failure deriving sessions.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("reading the bronze telemetry: {0}")]
    Query(#[from] medallion::QueryError),
}

/// How long a device has to go unheard before the silence separates two sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Gap(Duration);

impl Gap {
    pub fn new(silence: Duration) -> Self {
        Self(silence)
    }

    /// The threshold in whole seconds, as a session records the one it was derived under.
    pub fn as_seconds(self) -> u32 {
        self.0.num_seconds().try_into().unwrap_or(u32::MAX)
    }

    /// Whether `interval` is long enough to separate two sessions. An interval of exactly
    /// the threshold is not: the threshold is the longest silence a session survives.
    fn separates(self, interval: Duration) -> bool {
        interval > self.0
    }
}

impl Default for Gap {
    /// Long enough that a stop at a station, a signal or a tunnel does not end a journey,
    /// short enough that two journeys either side of an errand are not read as one.
    fn default() -> Self {
        Self(Duration::minutes(10))
    }
}

/// One GPS sample, deduped out of bronze.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub device_id: DeviceId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    pub t: DateTime<Utc>,
    pub lat: f64,
    pub lon: f64,
    pub alt: Option<f64>,
    pub acc: f64,
    pub speed: Option<f64>,
    pub heading: Option<f64>,
}

/// A device reporting that it has begun a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SessionStart {
    device_id: DeviceId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    t: DateTime<Utc>,
}

/// One contiguous run of samples from one device, and what began it.
///
/// A session holds at least one sample: it is the samples that make it, so a session start
/// nothing followed is not a session.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub device_id: DeviceId,
    pub started_by: StartedBy,
    /// The threshold this session was split at, carried so a session derived under one
    /// threshold stays interpretable after the default changes.
    pub gap: Gap,
    pub samples: Vec<Sample>,
}

impl Session {
    /// When the session began: the instant of its first sample.
    pub fn started_at(&self) -> DateTime<Utc> {
        self.samples
            .first()
            .expect("a session is built from the sample that starts it")
            .t
    }

    /// Where the session began: the position of its first sample. Which country that is
    /// in decides the zone its projected geometry is written in.
    pub fn started_from(&self) -> Point<f64> {
        let first = self
            .samples
            .first()
            .expect("a session is built from the sample that starts it");
        Point::new(first.lon, first.lat)
    }

    /// What identifies this session wherever it is written or read.
    pub fn id(&self) -> SessionId {
        SessionId::of(&self.device_id, self.started_at())
    }
}

/// Derive every session in the store, oldest first within each device.
///
/// A store holding no samples yet derives no sessions rather than failing: the datasets
/// are written by a separate drain, which may not have run.
pub async fn sessions(root: &Root, gap: Gap) -> Result<Vec<Session>, SessionError> {
    let query = Query::new(root.clone());
    if !query
        .register_if_present(model::GPS_READING, SAMPLES)
        .await?
    {
        return Ok(Vec::new());
    }
    let samples: Vec<Sample> = query.rows(DISTINCT_SAMPLES).await?;

    let started = if query
        .register_if_present(model::DEVICE_SESSION, SESSION_STARTS)
        .await?
    {
        started_by_device(query.rows(DISTINCT_SESSION_STARTS).await?)
    } else {
        HashMap::new()
    };

    Ok(samples
        .chunk_by(|a, b| a.device_id == b.device_id)
        .flat_map(|device| {
            let started = started
                .get(&device[0].device_id)
                .map(Vec::as_slice)
                .unwrap_or_default();
            split(device, started, gap)
        })
        .collect())
}

/// The instants each device reported a session start at, in time order.
fn started_by_device(starts: Vec<SessionStart>) -> HashMap<DeviceId, Vec<DateTime<Utc>>> {
    starts
        .into_iter()
        .fold(HashMap::new(), |mut by_device, start| {
            by_device.entry(start.device_id).or_default().push(start.t);
            by_device
        })
}

/// Split one device's `samples` into sessions, given the instants it reported a session
/// `started` at, both in time order.
///
/// A reported start takes effect at the first sample that follows it, so one no sample
/// follows produces nothing. A device that reports no start at all — as the earliest
/// protocol version could not — still produces sessions: its first sample starts one, and
/// a silence starts each of the rest.
fn split(samples: &[Sample], started: &[DateTime<Utc>], gap: Gap) -> Vec<Session> {
    let mut sessions: Vec<Session> = Vec::new();
    let mut unclaimed = started;
    let mut previous: Option<DateTime<Utc>> = None;

    for sample in samples {
        let claimed = unclaimed.partition_point(|start| *start <= sample.t);
        unclaimed = &unclaimed[claimed..];

        // A reported start is explicit, so it outranks both of the inferred reasons.
        let started_by = match previous {
            _ if claimed > 0 => Some(StartedBy::StartSession),
            None => Some(StartedBy::FirstSeen),
            Some(previous) if gap.separates(sample.t - previous) => Some(StartedBy::Gap),
            Some(_) => None,
        };
        previous = Some(sample.t);

        match started_by {
            Some(started_by) => sessions.push(Session {
                device_id: sample.device_id.clone(),
                started_by,
                gap,
                samples: vec![sample.clone()],
            }),
            // The first sample always starts a session, so by here one is running.
            None => sessions
                .last_mut()
                .expect("a sample that starts no session continues one")
                .samples
                .push(sample.clone()),
        }
    }

    sessions
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use shared::{DeviceInfo, DeviceType, Gps, GpsReading, Message, V1Message};
    use uuid::Uuid;

    use crate::bronze::{Archive, Payload};

    use super::*;

    /// The instant a test's first reading is taken at; later ones are offsets from it.
    fn start() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 26, 9, 0, 0).unwrap()
    }

    fn device(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn gps(id: Uuid, at: DateTime<Utc>, lat: f64) -> Message {
        Message::Version1(V1Message::Gps(GpsReading {
            id,
            t: at.timestamp_millis(),
            gps: Gps {
                lat,
                lon: 13.4,
                alt: Some(38.0),
                acc: 5.0,
                speed: Some(27.0),
                heading: Some(91.0),
            },
        }))
    }

    fn session_start(id: Uuid, at: DateTime<Utc>) -> Message {
        Message::Version1(V1Message::StartSession(shared::SessionStart {
            id,
            t: at.timestamp_millis(),
            device: DeviceInfo {
                device_type: DeviceType::Iphone,
                platform: "iPhone".into(),
                user_agent: "test".into(),
                os: Some("iOS".into()),
                os_version: Some("18.0".into()),
            },
        }))
    }

    /// A store holding `messages`, written through the archive the drain itself writes
    /// with, so the sessions are derived from bronze in the shape bronze really has.
    async fn store(tmp: &tempfile::TempDir, messages: &[Message]) -> Root {
        let root = Root::new(tmp.path());
        let json: Vec<String> = messages
            .iter()
            .map(|message| serde_json::to_string(message).expect("serialize"))
            .collect();
        let payloads: Vec<Payload> = json
            .iter()
            .map(|json| Payload {
                received_at: Some(start().timestamp_millis()),
                json,
            })
            .collect();

        Archive::new(root.clone())
            .write(start(), &payloads)
            .await
            .expect("archive");
        root
    }

    /// The sessions derived from a store holding `messages`, at the default threshold.
    async fn derived(tmp: &tempfile::TempDir, messages: &[Message]) -> Vec<Session> {
        derived_at(tmp, messages, Gap::default()).await
    }

    /// The same, at the threshold a test names rather than the default one.
    async fn derived_at(tmp: &tempfile::TempDir, messages: &[Message], gap: Gap) -> Vec<Session> {
        let root = store(tmp, messages).await;
        sessions(&root, gap).await.expect("derive sessions")
    }

    fn minutes(n: i64) -> Duration {
        Duration::minutes(n)
    }

    /// The earliest protocol version reported no session start at all, so samples with
    /// nothing preceding them still make a session.
    #[tokio::test]
    async fn samples_no_session_start_precedes_still_form_a_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = device(1);

        let sessions = derived(
            &tmp,
            &[
                gps(id, start(), 52.5),
                gps(id, start() + minutes(1), 52.6),
                gps(id, start() + minutes(2), 52.7),
            ],
        )
        .await;

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].started_by, StartedBy::FirstSeen);
        assert_eq!(sessions[0].samples.len(), 3);
        assert_eq!(sessions[0].device_id, DeviceId::from(id));
    }

    /// A silence longer than the threshold is a new trip; one shorter is the same trip
    /// continuing.
    #[tokio::test]
    async fn a_silence_longer_than_the_threshold_starts_a_new_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = device(1);

        let sessions = derived_at(
            &tmp,
            &[
                gps(id, start(), 52.5),
                gps(id, start() + minutes(9), 52.6),
                gps(id, start() + minutes(30), 52.7),
            ],
            Gap::new(minutes(10)),
        )
        .await;

        assert_eq!(
            sessions
                .iter()
                .map(|session| (session.started_by, session.samples.len()))
                .collect::<Vec<_>>(),
            [(StartedBy::FirstSeen, 2), (StartedBy::Gap, 1)]
        );
    }

    /// The threshold is the longest silence a session survives, so an interval of exactly
    /// it does not split.
    #[tokio::test]
    async fn a_silence_of_exactly_the_threshold_keeps_one_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = device(1);

        let sessions = derived_at(
            &tmp,
            &[gps(id, start(), 52.5), gps(id, start() + minutes(10), 52.6)],
            Gap::new(minutes(10)),
        )
        .await;

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].samples.len(), 2);
    }

    /// A reported start splits regardless of the interval, since it is the device saying
    /// so rather than an inference about it.
    #[tokio::test]
    async fn a_session_start_within_the_threshold_starts_a_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = device(1);

        let sessions = derived(
            &tmp,
            &[
                gps(id, start(), 52.5),
                session_start(id, start() + minutes(1)),
                gps(id, start() + minutes(2), 52.6),
            ],
        )
        .await;

        assert_eq!(
            sessions
                .iter()
                .map(|session| session.started_by)
                .collect::<Vec<_>>(),
            [StartedBy::FirstSeen, StartedBy::StartSession]
        );
    }

    /// A device that reports a start before its first sample has that session attributed to
    /// the report rather than to there being nothing before it.
    #[tokio::test]
    async fn a_session_start_before_the_first_sample_starts_that_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = device(1);

        let sessions = derived(
            &tmp,
            &[
                session_start(id, start()),
                gps(id, start() + minutes(1), 52.5),
            ],
        )
        .await;

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].started_by, StartedBy::StartSession);
    }

    /// A session is its samples, so a reported start nothing follows leaves no empty session
    /// for a reader to filter out.
    #[tokio::test]
    async fn a_session_start_no_sample_follows_produces_no_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = device(1);

        let sessions = derived(
            &tmp,
            &[
                gps(id, start(), 52.5),
                session_start(id, start() + minutes(1)),
            ],
        )
        .await;

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].samples.len(), 1);
    }

    /// A repeated sample is one sample: bronze allows the same observation to arrive
    /// twice, and the zero interval between the copies is not a reading taken instantly
    /// after another.
    #[tokio::test]
    async fn a_sample_arriving_twice_is_one_sample() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = device(1);

        let sessions = derived(
            &tmp,
            &[
                gps(id, start(), 52.5),
                gps(id, start(), 52.5),
                gps(id, start() + minutes(1), 52.6),
            ],
        )
        .await;

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].samples.len(), 2);
    }

    /// Devices are sessionised independently: one device's sample says nothing about whether
    /// another has gone quiet.
    #[tokio::test]
    async fn two_devices_recording_at_once_get_their_own_sessions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (first, second) = (device(1), device(2));

        let sessions = derived(
            &tmp,
            &[
                gps(first, start(), 52.5),
                gps(second, start() + minutes(1), 48.1),
                gps(first, start() + minutes(2), 52.6),
                gps(second, start() + minutes(30), 48.2),
            ],
        )
        .await;

        let devices: Vec<(DeviceId, usize)> = sessions
            .iter()
            .map(|session| (session.device_id.clone(), session.samples.len()))
            .collect();
        assert_eq!(
            devices,
            [
                (DeviceId::from(first), 2),
                (DeviceId::from(second), 1),
                (DeviceId::from(second), 1)
            ]
        );
    }

    /// The drain may not have run yet, which is a store with nothing in it rather than a
    /// failure.
    #[tokio::test]
    async fn a_store_with_no_samples_derives_no_sessions() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let derived = sessions(&Root::new(tmp.path()), Gap::default())
            .await
            .expect("derive sessions");

        assert!(derived.is_empty());
    }
}
