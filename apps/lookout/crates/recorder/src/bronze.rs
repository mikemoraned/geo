use chrono::{DateTime, Utc};
use medallion::{Dataset, DatasetSpec, Root, Row};
use medallion_model::{AccelReadingRow, DeviceSessionRow, GpsReadingRow, RawSampleRow};
use shared::{AccelReading, GpsReading, Message, SessionStart, V0Message, V1Message};
use telemetry::RawSample;

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

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("partitioning the dataset: {0}")]
    Path(#[from] medallion::PathError),
    #[error("writing the dataset: {0}")]
    Write(#[from] medallion::AppendError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Written {
    pub raw: usize,
    pub gps: usize,
    pub accel: usize,
    pub devices: usize,
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

#[derive(Debug, Default)]
struct Rows {
    raw: Vec<RawSampleRow>,
    gps: Vec<GpsReadingRow>,
    accel: Vec<AccelReadingRow>,
    devices: Vec<DeviceSessionRow>,
    unparseable: usize,
}

#[derive(Debug, Clone)]
pub struct Archive {
    root: Root,
}

impl Archive {
    pub fn new(root: Root) -> Self {
        Self { root }
    }

    fn partition<L: medallion::LayerKind>(
        &self,
        dataset: DatasetSpec<L>,
        ingested_at: DateTime<Utc>,
    ) -> Result<Dataset<L>, ArchiveError> {
        Ok(self
            .root
            .dataset(dataset)
            .on_date(ingested_at.date_naive())?)
    }

    #[cfg(test)]
    fn ingestion_file<L: medallion::LayerKind>(
        &self,
        dataset: DatasetSpec<L>,
        ingested_at: DateTime<Utc>,
    ) -> Result<std::path::PathBuf, ArchiveError> {
        Ok(self
            .partition(dataset, ingested_at)?
            .batch_file(ingested_at))
    }

    pub async fn write(
        &self,
        ingested_at: DateTime<Utc>,
        payloads: &[Payload<'_>],
    ) -> Result<Written, ArchiveError> {
        let rows = Rows::interpret(payloads);

        self.write_dataset(ingested_at, &rows.raw).await?;
        self.write_dataset(ingested_at, &rows.gps).await?;
        self.write_dataset(ingested_at, &rows.accel).await?;
        self.write_dataset(ingested_at, &rows.devices).await?;

        Ok(Written {
            raw: rows.raw.len(),
            gps: rows.gps.len(),
            accel: rows.accel.len(),
            devices: rows.devices.len(),
            unparseable: rows.unparseable,
        })
    }

    async fn write_dataset<T: Row>(
        &self,
        ingested_at: DateTime<Utc>,
        rows: &[T],
    ) -> Result<(), ArchiveError> {
        self.partition(T::DATASET, ingested_at)?
            .append_rows(ingested_at, rows)
            .await?;
        Ok(())
    }
}

impl Rows {
    fn interpret(payloads: &[Payload<'_>]) -> Self {
        let mut rows = Self::default();
        for payload in payloads {
            rows.raw.push(raw_row(payload));
            match serde_json::from_str::<Message>(payload.json) {
                Ok(Message::Version0(V0Message::Gps(r)) | Message::Version1(V1Message::Gps(r))) => {
                    rows.gps.extend(gps_row(&r))
                }
                Ok(
                    Message::Version0(V0Message::Acceleration(r))
                    | Message::Version1(V1Message::Acceleration(r)),
                ) => rows.accel.push(accel_row(&r)),
                Ok(Message::Version1(V1Message::StartSession(s))) => {
                    rows.devices.push(device_session_row(&s))
                }
                Err(_) => rows.unparseable += 1,
            }
        }
        rows
    }
}

fn raw_row(payload: &Payload<'_>) -> RawSampleRow {
    RawSampleRow {
        md5: format!("{:x}", md5::compute(payload.json)),
        received_at: payload.received_at,
        json: payload.json.to_string(),
    }
}

fn gps_row(reading: &GpsReading) -> Option<GpsReadingRow> {
    Some(GpsReadingRow {
        device_id: reading.id.into(),
        t: reading.t,
        lat: reading.gps.latitude(),
        lon: reading.gps.longitude(),
        alt: reading.gps.altitude_metres,
        acc: reading.gps.accuracy_metres?,
        speed: reading.gps.speed_mps,
        heading: reading.gps.heading_degrees,
    })
}

fn accel_row(reading: &AccelReading) -> AccelReadingRow {
    AccelReadingRow {
        device_id: reading.id.into(),
        t: reading.t,
        rms: reading.accel.rms,
        peak: reading.accel.peak,
        n: reading.accel.n,
        x: reading.accel.x,
        y: reading.accel.y,
        z: reading.accel.z,
    }
}

fn device_session_row(start: &SessionStart) -> DeviceSessionRow {
    DeviceSessionRow {
        device_id: start.id.into(),
        t: start.t,
        device_type: start.device.device_type,
        platform: start.device.platform.clone(),
        user_agent: start.device.user_agent.clone(),
        os: start.device.os.clone(),
        os_version: start.device.os_version.clone(),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use domain::Gps;
    use domain::{DeviceInfo, DeviceType};
    use medallion::Query;
    use shared::{Accel, AccelReading, GpsReading, SessionStart};
    use uuid::Uuid;

    use super::*;

    fn ingested_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 26, 14, 5, 30).unwrap()
    }

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
            gps: Gps::at(lat, -3.19)
                .expect("on the globe")
                .with_altitude_metres(Some(80.0))
                .with_accuracy_metres(Some(5.0))
                .with_speed_mps(Some(31.4))
                .with_heading_degrees(Some(275.0)),
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

    async fn rows_in<L: medallion::LayerKind>(root: &Root, dataset: DatasetSpec<L>) -> i64 {
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
        assert_eq!(rows_in(&root, medallion_model::RAW_SAMPLE).await, 4);
        assert_eq!(rows_in(&root, medallion_model::GPS_READING).await, 2);
        assert_eq!(rows_in(&root, medallion_model::ACCEL_READING).await, 1);
        assert_eq!(rows_in(&root, medallion_model::DEVICE_SESSION).await, 1);
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
            .ingestion_file(medallion_model::GPS_READING, ingested_at())
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
        assert_eq!(rows_in(&root, medallion_model::RAW_SAMPLE).await, 1);
    }

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
            .register(medallion_model::RAW_SAMPLE, "d")
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

        assert!(
            !archive
                .ingestion_file(medallion_model::ACCEL_READING, ingested_at())
                .expect("path")
                .exists()
        );
    }

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

        assert_eq!(rows_in(&root, medallion_model::GPS_READING).await, 2);
    }

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
        assert!(
            !archive
                .ingestion_file(medallion_model::RAW_SAMPLE, ingested_at())
                .expect("path")
                .exists()
        );
    }
}
