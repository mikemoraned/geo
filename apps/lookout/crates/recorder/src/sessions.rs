use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use domain::{DeviceId, SessionId, StartedBy};
use geo_types::Point;
use medallion::{Query, Root};
use serde::{Deserialize, Serialize};

const SAMPLES: &str = "samples";

const SESSION_STARTS: &str = "session_starts";

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

const DISTINCT_SESSION_STARTS: &str = "
    SELECT DISTINCT device_id, t FROM session_starts ORDER BY device_id, t
";

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("reading the bronze telemetry: {0}")]
    Query(#[from] medallion::QueryError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Gap(Duration);

impl Gap {
    pub fn new(silence: Duration) -> Self {
        Self(silence)
    }

    pub fn as_seconds(self) -> u32 {
        self.0.num_seconds().try_into().unwrap_or(u32::MAX)
    }

    fn separates(self, interval: Duration) -> bool {
        interval > self.0
    }
}

impl Default for Gap {
    fn default() -> Self {
        Self(Duration::minutes(10))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Lead(Duration);

impl Lead {
    pub fn new(before: Duration) -> Self {
        Self(before)
    }

    pub fn as_seconds(self) -> u32 {
        self.0.num_seconds().try_into().unwrap_or(u32::MAX)
    }

    fn reaches(self, announced_at: DateTime<Utc>, started_at: DateTime<Utc>) -> bool {
        started_at >= announced_at - self.0
    }
}

impl Default for Lead {
    fn default() -> Self {
        Self(Duration::minutes(1))
    }
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SessionStart {
    device_id: DeviceId,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    t: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub device_id: DeviceId,
    pub started_by: StartedBy,
    pub gap: Gap,
    pub lead: Lead,
    pub samples: Vec<Sample>,
}

impl Session {
    pub fn started_at(&self) -> DateTime<Utc> {
        self.samples
            .first()
            .expect("a session is built from the sample that starts it")
            .t
    }

    pub fn started_from(&self) -> Point<f64> {
        let first = self
            .samples
            .first()
            .expect("a session is built from the sample that starts it");
        Point::new(first.lon, first.lat)
    }

    pub fn id(&self) -> SessionId {
        SessionId::of(&self.device_id, self.started_at())
    }
}

pub async fn sessions(root: &Root, gap: Gap, lead: Lead) -> Result<Vec<Session>, SessionError> {
    let query = Query::new(root.clone());
    if !query
        .register_if_present(medallion_model::GPS_READING, SAMPLES)
        .await?
    {
        return Ok(Vec::new());
    }
    let samples: Vec<Sample> = query.rows(DISTINCT_SAMPLES).await?;

    let started = if query
        .register_if_present(medallion_model::DEVICE_SESSION, SESSION_STARTS)
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
            split(device, started, gap, lead)
        })
        .collect())
}

fn started_by_device(starts: Vec<SessionStart>) -> HashMap<DeviceId, Vec<DateTime<Utc>>> {
    starts
        .into_iter()
        .fold(HashMap::new(), |mut by_device, start| {
            by_device.entry(start.device_id).or_default().push(start.t);
            by_device
        })
}

fn absorbed(sessions: &mut Vec<Session>, announced_at: DateTime<Utc>, lead: Lead) -> Vec<Sample> {
    let precedes_report = sessions
        .last()
        .is_some_and(|previous| lead.reaches(announced_at, previous.started_at()));

    if precedes_report {
        sessions
            .pop()
            .expect("a session was just read from the end")
            .samples
    } else {
        Vec::new()
    }
}

fn split(samples: &[Sample], started: &[DateTime<Utc>], gap: Gap, lead: Lead) -> Vec<Session> {
    let mut sessions: Vec<Session> = Vec::new();
    let mut unclaimed = started;
    let mut previous: Option<DateTime<Utc>> = None;

    for sample in samples {
        let claimed = unclaimed.partition_point(|start| *start <= sample.t);
        let announced_at = unclaimed[..claimed].first().copied();
        unclaimed = &unclaimed[claimed..];

        let started_by = match previous {
            _ if claimed > 0 => Some(StartedBy::StartSession),
            None => Some(StartedBy::FirstSeen),
            Some(previous) if gap.separates(sample.t - previous) => Some(StartedBy::Gap),
            Some(_) => None,
        };
        previous = Some(sample.t);

        match started_by {
            Some(started_by) => {
                let mut samples = announced_at
                    .map(|announced_at| absorbed(&mut sessions, announced_at, lead))
                    .unwrap_or_default();
                samples.push(sample.clone());
                sessions.push(Session {
                    device_id: sample.device_id.clone(),
                    started_by,
                    gap,
                    lead,
                    samples,
                });
            }
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
    use domain::Gps;
    use domain::{DeviceInfo, DeviceType};
    use shared::{GpsReading, Message, V1Message};
    use uuid::Uuid;

    use crate::bronze::{Archive, Payload};

    use super::*;

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
            gps: Gps::at(lat, 13.4)
                .expect("on the globe")
                .with_altitude_metres(Some(38.0))
                .with_accuracy_metres(Some(5.0))
                .with_speed_mps(Some(27.0))
                .with_heading_degrees(Some(91.0)),
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

    async fn derived(tmp: &tempfile::TempDir, messages: &[Message]) -> Vec<Session> {
        derived_at(tmp, messages, Gap::default()).await
    }

    async fn derived_at(tmp: &tempfile::TempDir, messages: &[Message], gap: Gap) -> Vec<Session> {
        let root = store(tmp, messages).await;
        sessions(&root, gap, Lead::default())
            .await
            .expect("derive sessions")
    }

    fn minutes(n: i64) -> Duration {
        Duration::minutes(n)
    }

    fn seconds(n: i64) -> Duration {
        Duration::seconds(n)
    }

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

    #[tokio::test]
    async fn a_session_start_within_the_threshold_starts_a_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = device(1);

        let sessions = derived(
            &tmp,
            &[
                gps(id, start(), 52.5),
                gps(id, start() + minutes(2), 52.6),
                session_start(id, start() + minutes(3)),
                gps(id, start() + minutes(4), 52.7),
            ],
        )
        .await;

        assert_eq!(
            sessions
                .iter()
                .map(|session| (session.started_by, session.samples.len()))
                .collect::<Vec<_>>(),
            [(StartedBy::FirstSeen, 2), (StartedBy::StartSession, 1)]
        );
    }

    #[tokio::test]
    async fn a_sample_taken_just_before_a_report_opens_the_session_it_reports() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = device(1);

        let sessions = derived(
            &tmp,
            &[
                gps(id, start(), 52.5),
                session_start(id, start() + seconds(8)),
                gps(id, start() + seconds(14), 52.6),
                gps(id, start() + seconds(20), 52.7),
            ],
        )
        .await;

        assert_eq!(sessions.len(), 1, "one journey, not a stub and a journey");
        assert_eq!(sessions[0].started_by, StartedBy::StartSession);
        assert_eq!(sessions[0].samples.len(), 3);
        assert_eq!(
            sessions[0].started_at(),
            start(),
            "the absorbed sample is the first thing recorded of the journey"
        );
    }

    #[tokio::test]
    async fn a_sample_long_before_a_report_stays_its_own_session() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = device(1);

        let sessions = derived(
            &tmp,
            &[
                gps(id, start(), 52.5),
                session_start(id, start() + minutes(30)),
                gps(id, start() + minutes(30) + seconds(6), 52.6),
            ],
        )
        .await;

        assert_eq!(
            sessions
                .iter()
                .map(|session| (session.started_by, session.samples.len()))
                .collect::<Vec<_>>(),
            [(StartedBy::FirstSeen, 1), (StartedBy::StartSession, 1)]
        );
    }

    #[tokio::test]
    async fn several_samples_before_a_report_all_open_the_session_it_reports() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let id = device(1);

        let sessions = derived(
            &tmp,
            &[
                gps(id, start(), 52.5),
                gps(id, start() + seconds(5), 52.6),
                gps(id, start() + seconds(10), 52.7),
                session_start(id, start() + seconds(12)),
                gps(id, start() + seconds(18), 52.8),
            ],
        )
        .await;

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].samples.len(), 4);
        assert_eq!(sessions[0].started_at(), start());
    }

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

    #[tokio::test]
    async fn a_store_with_no_samples_derives_no_sessions() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let derived = sessions(&Root::new(tmp.path()), Gap::default(), Lead::default())
            .await
            .expect("derive sessions");

        assert!(derived.is_empty());
    }
}
