//! The versioned telemetry message model shared by the `server` (which receives
//! messages over a websocket and queues them) and the `recorder` cli (which drains
//! the queue and interprets them).
//!
//! # Wire shape
//!
//! Every message carries a protocol version in a top-level `v` field. It is a plain
//! number; **absent means `0`** (`#[serde(default)]` semantics), so the original
//! unversioned payloads still in the `raw` archive keep parsing.
//!
//! - **Version 0** (`v` absent) has no message-type tag: the variant is inferred
//!   from which sensor key is present (`gps` vs `accel`). Only [`GpsReading`] and
//!   [`AccelReading`] exist.
//!
//!   ```json
//!   {"id":"…","t":1700000000000,"gps":{"lat":…,"lon":…,"alt":null,"acc":…}}
//!   {"id":"…","t":1700000000000,"accel":{"x":…,"y":…,"z":…}}
//!   ```
//!
//! - **Version 1** (`v:1`, recorded explicitly) is self-describing: a `type` tag
//!   selects the variant, adding [`SessionStart`] alongside the two readings.
//!
//!   ```json
//!   {"v":1,"type":"start_session","id":"…","t":…,"device":{…}}
//!   {"v":1,"type":"gps","id":"…","t":…,"gps":{…}}
//!   {"v":1,"type":"acceleration","id":"…","t":…,"accel":{…}}
//!   ```

use serde::de::Error as _;
use serde::ser::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use uuid::Uuid;

use crate::sensor::{Accel, Gps};
use crate::session::DeviceInfo;

/// A GPS reading from a device at a point in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpsReading {
    /// Stable per-device identity (a `crypto.randomUUID()` persisted in a cookie).
    pub id: Uuid,
    /// Capture time as epoch milliseconds.
    pub t: i64,
    pub gps: Gps,
}

/// An accelerometer reading from a device at a point in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccelReading {
    pub id: Uuid,
    pub t: i64,
    pub accel: Accel,
}

/// The start of a recording session: a device announcing its identity and metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionStart {
    pub id: Uuid,
    pub t: i64,
    pub device: DeviceInfo,
}

/// The version-0 message set. Untagged: the variant is inferred from the sensor key
/// present, since the historical payloads carry no type tag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum V0Message {
    Gps(GpsReading),
    Acceleration(AccelReading),
}

/// The version-1 message set. Internally tagged on `type`, adding [`SessionStart`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum V1Message {
    StartSession(SessionStart),
    Gps(GpsReading),
    Acceleration(AccelReading),
}

/// A telemetry message, tagged by protocol version. See the [module docs](self) for
/// the wire shape and how the version is carried.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Version0(V0Message),
    Version1(V1Message),
}

impl Message {
    /// The device id every message variant carries.
    pub fn id(&self) -> Uuid {
        match self {
            Message::Version0(V0Message::Gps(r)) => r.id,
            Message::Version0(V0Message::Acceleration(r)) => r.id,
            Message::Version1(V1Message::StartSession(s)) => s.id,
            Message::Version1(V1Message::Gps(r)) => r.id,
            Message::Version1(V1Message::Acceleration(r)) => r.id,
        }
    }

    /// The capture timestamp (epoch millis) every message variant carries.
    pub fn t(&self) -> i64 {
        match self {
            Message::Version0(V0Message::Gps(r)) => r.t,
            Message::Version0(V0Message::Acceleration(r)) => r.t,
            Message::Version1(V1Message::StartSession(s)) => s.t,
            Message::Version1(V1Message::Gps(r)) => r.t,
            Message::Version1(V1Message::Acceleration(r)) => r.t,
        }
    }
}

/// The current protocol version emitted by clients. Version 0 is read-only history.
const CURRENT_VERSION: u64 = 1;

impl Serialize for Message {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            // Version 0 has no `v` tag on the wire — it's the absent-means-0 default.
            Message::Version0(message) => message.serialize(serializer),
            // Version 1 is recorded explicitly, so stamp `v` onto the tagged body.
            Message::Version1(message) => {
                let mut value = serde_json::to_value(message).map_err(S::Error::custom)?;
                if let Value::Object(map) = &mut value {
                    map.insert("v".to_string(), Value::from(CURRENT_VERSION));
                }
                value.serialize(serializer)
            }
        }
    }
}

impl<'de> Deserialize<'de> for Message {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let version = value.get("v").and_then(Value::as_u64).unwrap_or(0);
        match version {
            0 => serde_json::from_value(value)
                .map(Message::Version0)
                .map_err(D::Error::custom),
            1 => serde_json::from_value(value)
                .map(Message::Version1)
                .map_err(D::Error::custom),
            other => Err(D::Error::custom(format!("unknown message version {other}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::DeviceType;

    fn gps_reading() -> GpsReading {
        GpsReading {
            id: Uuid::from_u128(1),
            t: 1_700_000_000_000,
            gps: Gps {
                lat: 55.95,
                lon: -3.19,
                alt: None,
                acc: 8.5,
                speed: Some(31.4),
                heading: None,
            },
        }
    }

    fn accel_reading() -> AccelReading {
        AccelReading {
            id: Uuid::from_u128(2),
            t: 1_700_000_000_001,
            accel: Accel {
                rms: 0.42,
                peak: 1.7,
                n: 600,
                x: Some(0.1),
                y: Some(-9.8),
                z: Some(0.3),
            },
        }
    }

    fn session_start() -> SessionStart {
        SessionStart {
            id: Uuid::from_u128(3),
            t: 1_700_000_000_002,
            device: DeviceInfo {
                device_type: DeviceType::Iphone,
                platform: "iPhone".to_string(),
                user_agent: "Mozilla/5.0 (iPhone; …) Safari".to_string(),
                os: Some("iOS".to_string()),
                os_version: Some("18.5".to_string()),
            },
        }
    }

    fn roundtrip(message: &Message) -> Message {
        let json = serde_json::to_string(message).expect("serialize");
        serde_json::from_str(&json).expect("deserialize")
    }

    #[test]
    fn version0_variants_roundtrip() {
        for message in [
            Message::Version0(V0Message::Gps(gps_reading())),
            Message::Version0(V0Message::Acceleration(accel_reading())),
        ] {
            assert_eq!(roundtrip(&message), message);
        }
    }

    #[test]
    fn version1_variants_roundtrip() {
        for message in [
            Message::Version1(V1Message::StartSession(session_start())),
            Message::Version1(V1Message::Gps(gps_reading())),
            Message::Version1(V1Message::Acceleration(accel_reading())),
        ] {
            assert_eq!(roundtrip(&message), message);
        }
    }

    /// A payload without a `v` field decodes as Version0.
    #[test]
    fn absent_version_decodes_as_v0() {
        let json = r#"{"id":"00000000-0000-0000-0000-000000000001","t":1700000000000,"gps":{"lat":55.95,"lon":-3.19,"alt":null,"acc":8.5,"speed":31.4,"heading":null}}"#;
        let message: Message = serde_json::from_str(json).expect("deserialize");
        assert_eq!(message, Message::Version0(V0Message::Gps(gps_reading())));
    }

    /// An explicit `v:1` decodes as Version1.
    #[test]
    fn explicit_version_1_decodes_as_v1() {
        let json = r#"{"v":1,"type":"gps","id":"00000000-0000-0000-0000-000000000001","t":1700000000000,"gps":{"lat":55.95,"lon":-3.19,"alt":null,"acc":8.5,"speed":31.4,"heading":null}}"#;
        let message: Message = serde_json::from_str(json).expect("deserialize");
        assert_eq!(message, Message::Version1(V1Message::Gps(gps_reading())));
    }

    /// Version 1 serialization stamps `v:1` and the `type` tag onto the wire.
    #[test]
    fn version1_serializes_with_v_and_type() {
        let message = Message::Version1(V1Message::Acceleration(accel_reading()));
        let value: Value = serde_json::to_value(&message).expect("serialize");
        assert_eq!(value["v"], Value::from(1));
        assert_eq!(value["type"], Value::from("acceleration"));
    }

    /// Version 0 serialization carries no `v` tag (absent-means-0 on the wire).
    #[test]
    fn version0_serializes_without_v() {
        let message = Message::Version0(V0Message::Gps(gps_reading()));
        let json = serde_json::to_string(&message).expect("serialize");
        assert!(!json.contains("\"v\""), "v0 must not carry a v tag: {json}");
        assert!(!json.contains("type"), "v0 must not carry a type tag: {json}");
    }

    /// The exact v0 wire shapes stored in `raw` must still parse — re-interpretation
    /// from the lossless archive can't break on the version refactor.
    #[test]
    fn historical_raw_v0_shapes_parse() {
        let stored_gps = r#"{"id":"00000000-0000-0000-0000-000000000001","t":1700000000000,"gps":{"lat":55.95,"lon":-3.19,"alt":80.0,"acc":5.0}}"#;
        let stored_accel = r#"{"id":"00000000-0000-0000-0000-000000000001","t":1700000000000,"accel":{"x":0.1,"y":-9.8,"z":0.3}}"#;

        let gps: Message = serde_json::from_str(stored_gps).expect("parse stored gps");
        let Message::Version0(V0Message::Gps(r)) = gps else {
            panic!("expected v0 gps, got {gps:?}");
        };
        // The fields v0 never carried default rather than failing to parse.
        assert_eq!(r.gps.speed, None);
        assert_eq!(r.gps.heading, None);

        let accel: Message = serde_json::from_str(stored_accel).expect("parse stored accel");
        let Message::Version0(V0Message::Acceleration(r)) = accel else {
            panic!("expected v0 accel, got {accel:?}");
        };
        assert_eq!(r.accel.n, 0);
        assert_eq!(r.accel.rms, 0.0);
        assert_eq!(r.accel.x, Some(0.1));
    }

    #[test]
    fn unknown_version_is_rejected() {
        let json = r#"{"v":99,"id":"00000000-0000-0000-0000-000000000001","t":1700000000000}"#;
        let result: Result<Message, _> = serde_json::from_str(json);
        assert!(result.is_err(), "unknown version must not decode");
    }
}
