//! The bronze telemetry datasets, written one file per ingestion.
//!
//! An ingestion writes four datasets, each partitioned by the UTC date it was ingested on
//! and named for the instant of the write:
//!
//!   - `raw_sample` — every payload verbatim, keyed on its md5. This is the lossless
//!     record everything else is derived from, so a payload that fails to parse still
//!     lands here.
//!   - `gps_reading` / `accel_reading` — one row per reading, interpreted from the
//!     payloads. Both protocol versions produce the same rows.
//!   - `device_session` — the metadata a device announces when it starts a session.
//!
//! Readings are split by sensor into their own datasets rather than sharing one under a
//! `sensor=` partition, because the two carry different columns and a dataset is one
//! schema.

use chrono::{DateTime, Utc};
use medallion::{Dataset, DatasetSpec, Root};
use serde::{Deserialize, Serialize};
use shared::{AccelReading, GpsReading, Message, SessionStart, V0Message, V1Message};
use telemetry::RawSample;

/// One payload to archive: the json exactly as it arrived, and the epoch millis the server
/// stamped when it received it.
///
/// `received_at` is optional because a payload restored from an older archive may predate
/// receipt times being recorded at all; a payload off the queue always carries one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Payload<'a> {
    pub received_at: Option<i64>,
    pub json: &'a str,
}

impl<'a> From<&'a RawSample> for Payload<'a> {
    fn from(sample: &'a RawSample) -> Self {
        Self {
            received_at: Some(sample.received_at()),
            json: sample.json(),
        }
    }
}

/// Failure writing an ingestion.
#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("partitioning the dataset: {0}")]
    Path(#[from] medallion::PathError),
    #[error("writing the dataset: {0}")]
    Write(#[from] medallion::AppendError),
}

/// What one ingestion wrote. Sums, so a run made of several ingestions reports its total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Written {
    pub raw: usize,
    pub gps: usize,
    pub accel: usize,
    pub devices: usize,
    /// Payloads archived verbatim that no version of the protocol could interpret.
    pub unparseable: usize,
}

impl std::ops::Add for Written {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            raw: self.raw + other.raw,
            gps: self.gps + other.gps,
            accel: self.accel + other.accel,
            devices: self.devices + other.devices,
            unparseable: self.unparseable + other.unparseable,
        }
    }
}

/// One archived payload, exactly as it arrived.
///
/// Instants travel as epoch milliseconds — the form the wire carries them in — and are
/// declared as timestamp columns by [`fields`], so no conversion can narrow them.
#[derive(Debug, Serialize, Deserialize)]
struct RawRow {
    /// Identifies the payload, so re-ingesting the same one is recognisable downstream.
    md5: String,
    /// When the server stamped it on receipt, where that was recorded.
    received_at: Option<i64>,
    json: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GpsRow {
    device_id: String,
    t: i64,
    lat: f64,
    lon: f64,
    alt: Option<f64>,
    acc: f64,
    speed: Option<f64>,
    heading: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccelRow {
    device_id: String,
    t: i64,
    rms: f64,
    peak: f64,
    n: u32,
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeviceRow {
    device_id: String,
    t: i64,
    device_type: String,
    platform: String,
    user_agent: String,
    os: Option<String>,
    os_version: Option<String>,
}

/// The rows one ingestion interpreted, before they are written.
#[derive(Debug, Default)]
struct Rows {
    raw: Vec<RawRow>,
    gps: Vec<GpsRow>,
    accel: Vec<AccelRow>,
    devices: Vec<DeviceRow>,
    unparseable: usize,
}

/// A handle on the bronze telemetry datasets within a medallion store.
#[derive(Debug, Clone)]
pub struct Archive {
    root: Root,
}

impl Archive {
    pub fn new(root: Root) -> Self {
        Self { root }
    }

    /// The partition an ingestion at `ingested_at` writes `dataset` into.
    fn partition(
        &self,
        dataset: DatasetSpec,
        ingested_at: DateTime<Utc>,
    ) -> Result<Dataset, ArchiveError> {
        Ok(self
            .root
            .dataset(dataset)
            .on_date(ingested_at.date_naive())?)
    }

    /// The file an ingestion at `ingested_at` writes `dataset` to. Readers query the
    /// dataset rather than opening its files, so this is only the layout the tests assert
    /// on.
    #[cfg(test)]
    fn ingestion_file(
        &self,
        dataset: DatasetSpec,
        ingested_at: DateTime<Utc>,
    ) -> Result<std::path::PathBuf, ArchiveError> {
        Ok(self
            .partition(dataset, ingested_at)?
            .batch_file(ingested_at))
    }

    /// Interpret `payloads` and write them, returning what landed. Each dataset with no
    /// rows is skipped, so an ingestion of only GPS leaves no empty accel file.
    pub async fn write(
        &self,
        ingested_at: DateTime<Utc>,
        payloads: &[Payload<'_>],
    ) -> Result<Written, ArchiveError> {
        let rows = Rows::interpret(payloads);

        self.write_dataset(model::RAW_SAMPLE, ingested_at, &rows.raw, &["received_at"])
            .await?;
        self.write_dataset(model::GPS_READING, ingested_at, &rows.gps, &["t"])
            .await?;
        self.write_dataset(model::ACCEL_READING, ingested_at, &rows.accel, &["t"])
            .await?;
        self.write_dataset(model::DEVICE_SESSION, ingested_at, &rows.devices, &["t"])
            .await?;

        Ok(Written {
            raw: rows.raw.len(),
            gps: rows.gps.len(),
            accel: rows.accel.len(),
            devices: rows.devices.len(),
            unparseable: rows.unparseable,
        })
    }

    async fn write_dataset<T>(
        &self,
        dataset: DatasetSpec,
        ingested_at: DateTime<Utc>,
        rows: &[T],
        instants: &[&str],
    ) -> Result<(), ArchiveError>
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        self.partition(dataset, ingested_at)?
            .append_rows(ingested_at, rows, instants)
            .await?;
        Ok(())
    }
}

impl Rows {
    /// Split `payloads` into the rows each dataset holds. Every payload lands in `raw`,
    /// whether or not it can be interpreted.
    fn interpret(payloads: &[Payload<'_>]) -> Self {
        let mut rows = Self::default();
        for payload in payloads {
            rows.raw.push(RawRow::from(payload));
            match serde_json::from_str::<Message>(payload.json) {
                Ok(Message::Version0(V0Message::Gps(r)) | Message::Version1(V1Message::Gps(r))) => {
                    rows.gps.push(GpsRow::from(&r))
                }
                Ok(
                    Message::Version0(V0Message::Acceleration(r))
                    | Message::Version1(V1Message::Acceleration(r)),
                ) => rows.accel.push(AccelRow::from(&r)),
                Ok(Message::Version1(V1Message::StartSession(s))) => {
                    rows.devices.push(DeviceRow::from(&s))
                }
                Err(_) => rows.unparseable += 1,
            }
        }
        rows
    }
}

impl From<&Payload<'_>> for RawRow {
    fn from(payload: &Payload<'_>) -> Self {
        Self {
            md5: format!("{:x}", md5::compute(payload.json)),
            received_at: payload.received_at,
            json: payload.json.to_string(),
        }
    }
}

impl From<&GpsReading> for GpsRow {
    fn from(r: &GpsReading) -> Self {
        Self {
            device_id: r.id.to_string(),
            t: r.t,
            lat: r.gps.lat,
            lon: r.gps.lon,
            alt: r.gps.alt,
            acc: r.gps.acc,
            speed: r.gps.speed,
            heading: r.gps.heading,
        }
    }
}

impl From<&AccelReading> for AccelRow {
    fn from(r: &AccelReading) -> Self {
        Self {
            device_id: r.id.to_string(),
            t: r.t,
            rms: r.accel.rms,
            peak: r.accel.peak,
            n: r.accel.n,
            x: r.accel.x,
            y: r.accel.y,
            z: r.accel.z,
        }
    }
}

impl From<&SessionStart> for DeviceRow {
    fn from(s: &SessionStart) -> Self {
        Self {
            device_id: s.id.to_string(),
            t: s.t,
            device_type: s.device.device_type.as_str().to_string(),
            platform: s.device.platform.clone(),
            user_agent: s.device.user_agent.clone(),
            os: s.device.os.clone(),
            os_version: s.device.os_version.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use medallion::Query;
    use shared::{Accel, AccelReading, DeviceInfo, DeviceType, Gps, GpsReading, SessionStart};
    use uuid::Uuid;

    use super::*;

    /// The instant every test ingests at, so the file it writes is predictable.
    fn ingested_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 30).unwrap()
    }

    /// The payloads `samples` archive as, exercising the conversion the drain does.
    fn archived(samples: &[RawSample]) -> Vec<Payload<'_>> {
        samples.iter().map(Payload::from).collect()
    }

    fn queued(message: &Message) -> RawSample {
        RawSample::new(
            1_700_000_050_000,
            serde_json::to_string(message).expect("serialize"),
        )
    }

    fn gps(t: i64, lat: f64) -> Message {
        Message::Version1(V1Message::Gps(GpsReading {
            id: Uuid::from_u128(1),
            t,
            gps: Gps {
                lat,
                lon: -3.19,
                alt: Some(80.0),
                acc: 5.0,
                speed: Some(31.4),
                heading: Some(275.0),
            },
        }))
    }

    fn accel(t: i64) -> Message {
        Message::Version1(V1Message::Acceleration(AccelReading {
            id: Uuid::from_u128(1),
            t,
            accel: Accel {
                rms: 0.42,
                peak: 1.7,
                n: 600,
                x: Some(0.1),
                y: Some(-9.8),
                z: Some(0.3),
            },
        }))
    }

    fn session() -> Message {
        Message::Version1(V1Message::StartSession(SessionStart {
            id: Uuid::from_u128(1),
            t: 1_700_000_000_000,
            device: DeviceInfo {
                device_type: DeviceType::Iphone,
                platform: "iPhone".into(),
                user_agent: "test".into(),
                os: Some("iOS".into()),
                os_version: Some("18.0".into()),
            },
        }))
    }

    /// How many rows a written dataset holds, read back through SQL.
    async fn rows_in(root: &Root, dataset: DatasetSpec) -> i64 {
        let query = Query::new(root.clone());
        query
            .register(dataset, "d")
            .await
            .expect("register dataset");
        query
            .count("SELECT COUNT(*) AS count FROM d")
            .await
            .expect("count")
    }

    #[tokio::test]
    async fn every_payload_lands_raw_and_each_reading_in_its_own_dataset() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let samples = [
            queued(&session()),
            queued(&gps(1_700_000_000_001, 55.95)),
            queued(&gps(1_700_000_000_002, 55.96)),
            queued(&accel(1_700_000_000_003)),
        ];

        let written = Archive::new(root.clone())
            .write(ingested_at(), &archived(&samples))
            .await
            .expect("write");

        assert_eq!(
            written,
            Written {
                raw: 4,
                gps: 2,
                accel: 1,
                devices: 1,
                unparseable: 0
            }
        );
        assert_eq!(rows_in(&root, model::RAW_SAMPLE).await, 4);
        assert_eq!(rows_in(&root, model::GPS_READING).await, 2);
        assert_eq!(rows_in(&root, model::ACCEL_READING).await, 1);
        assert_eq!(rows_in(&root, model::DEVICE_SESSION).await, 1);
    }

    #[tokio::test]
    async fn an_ingestion_writes_one_file_per_dataset_named_for_its_instant() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let archive = Archive::new(Root::new(tmp.path()));

        archive
            .write(
                ingested_at(),
                &archived(&[queued(&gps(1_700_000_000_001, 55.95))]),
            )
            .await
            .expect("write");

        let path = archive
            .ingestion_file(model::GPS_READING, ingested_at())
            .expect("path");
        assert!(
            path.ends_with(
                "bronze/gps_reading/ingested_date=2026-07-26/20260726T140530000Z.parquet"
            ),
            "unexpected path: {}",
            path.display()
        );
        assert!(path.exists());
    }

    /// A payload no protocol version can interpret is still archived verbatim, since raw
    /// is what everything else is rederived from.
    #[tokio::test]
    async fn an_uninterpretable_payload_is_still_archived() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let samples = [RawSample::new(
            1_700_000_050_000,
            String::from("{\"not\":\"a message\"}"),
        )];

        let written = Archive::new(root.clone())
            .write(ingested_at(), &archived(&samples))
            .await
            .expect("write");

        assert_eq!(written.raw, 1);
        assert_eq!(written.unparseable, 1);
        assert_eq!(rows_in(&root, model::RAW_SAMPLE).await, 1);
    }

    /// A payload whose receipt was never timed is archived with that unknown, rather than
    /// with a stand-in instant that would read as a real one.
    #[tokio::test]
    async fn a_payload_with_no_receipt_time_is_archived_without_one() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let json = serde_json::to_string(&gps(1_700_000_000_001, 55.95)).expect("serialize");

        Archive::new(root.clone())
            .write(
                ingested_at(),
                &[Payload {
                    received_at: None,
                    json: &json,
                }],
            )
            .await
            .expect("write");

        let query = Query::new(root.clone());
        query
            .register(model::RAW_SAMPLE, "d")
            .await
            .expect("register");
        assert_eq!(
            query
                .count("SELECT COUNT(*) AS count FROM d WHERE received_at IS NULL")
                .await
                .expect("count"),
            1
        );
    }

    /// A dataset with no rows is skipped, so an ingestion of only GPS leaves no empty
    /// accel file for a reader to trip over.
    #[tokio::test]
    async fn datasets_with_no_rows_are_not_written() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let archive = Archive::new(Root::new(tmp.path()));

        archive
            .write(
                ingested_at(),
                &archived(&[queued(&gps(1_700_000_000_001, 55.95))]),
            )
            .await
            .expect("write");

        assert!(!archive
            .ingestion_file(model::ACCEL_READING, ingested_at())
            .expect("path")
            .exists());
    }

    /// A drain writes a batch at a time, so consecutive ingestions fall milliseconds apart:
    /// each must keep its own rows rather than the later one displacing the earlier.
    #[tokio::test]
    async fn ingestions_milliseconds_apart_both_keep_their_readings() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let archive = Archive::new(root.clone());

        archive
            .write(
                ingested_at(),
                &archived(&[queued(&gps(1_700_000_000_001, 55.95))]),
            )
            .await
            .expect("first batch");
        archive
            .write(
                ingested_at() + chrono::Duration::milliseconds(1),
                &archived(&[queued(&gps(1_700_000_000_002, 55.96))]),
            )
            .await
            .expect("second batch");

        assert_eq!(rows_in(&root, model::GPS_READING).await, 2);
    }

    /// Several ingestions sum, so a run made of batches reports its total.
    #[tokio::test]
    async fn what_each_ingestion_wrote_adds_up() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let archive = Archive::new(Root::new(tmp.path()));
        let first = archive
            .write(
                ingested_at(),
                &archived(&[queued(&gps(1_700_000_000_001, 55.95))]),
            )
            .await
            .expect("first");
        let second = archive
            .write(
                ingested_at() + chrono::Duration::seconds(1),
                &archived(&[queued(&gps(1_700_000_000_002, 55.96)), queued(&accel(3))]),
            )
            .await
            .expect("second");

        assert_eq!(
            first + second,
            Written {
                raw: 3,
                gps: 2,
                accel: 1,
                devices: 0,
                unparseable: 0
            }
        );
    }

    #[tokio::test]
    async fn ingesting_nothing_writes_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let archive = Archive::new(Root::new(tmp.path()));

        let written = archive.write(ingested_at(), &[]).await.expect("write");

        assert_eq!(written, Written::default());
        assert!(!archive
            .ingestion_file(model::RAW_SAMPLE, ingested_at())
            .expect("path")
            .exists());
    }
}
