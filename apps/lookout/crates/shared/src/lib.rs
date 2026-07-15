//! Telemetry sample model shared by the `server` (which receives samples over a
//! websocket and queues them) and the `recorder` cli (which drains the queue).
//!
//! A sample carries a device identity, a capture timestamp, and a reading from
//! either source. `gps` and `accel` are independent (they arrive at different
//! rates), so each is optional and a sample may carry either or both.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One telemetry sample from a device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Stable per-device identity (a `crypto.randomUUID()` persisted in a cookie).
    pub id: Uuid,
    /// Capture time as epoch milliseconds (`Date.now()` on the frontend).
    pub t: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gps: Option<Gps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accel: Option<Accel>,
}

/// A GPS fix. `alt` is optional because `Geolocation` may report a null altitude.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gps {
    pub lat: f64,
    pub lon: f64,
    pub alt: Option<f64>,
    /// Accuracy of the fix, in metres.
    pub acc: f64,
}

/// An accelerometer reading. Each axis is optional because `DeviceMotionEvent`
/// may report null components on devices without a full accelerometer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Accel {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(sample: &Sample) -> Sample {
        let json = serde_json::to_string(sample).expect("serialize");
        serde_json::from_str(&json).expect("deserialize")
    }

    #[test]
    fn roundtrips_accel_only() {
        let sample = Sample {
            id: Uuid::from_u128(1),
            t: 1_700_000_000_000,
            gps: None,
            accel: Some(Accel {
                x: Some(0.1),
                y: Some(-9.8),
                z: Some(0.3),
            }),
        };
        assert_eq!(roundtrip(&sample), sample);
    }

    #[test]
    fn roundtrips_gps_only() {
        let sample = Sample {
            id: Uuid::from_u128(2),
            t: 1_700_000_000_001,
            gps: Some(Gps {
                lat: 55.95,
                lon: -3.19,
                alt: None,
                acc: 12.0,
            }),
            accel: None,
        };
        assert_eq!(roundtrip(&sample), sample);
    }

    #[test]
    fn roundtrips_both() {
        let sample = Sample {
            id: Uuid::from_u128(3),
            t: 1_700_000_000_002,
            gps: Some(Gps {
                lat: 55.95,
                lon: -3.19,
                alt: Some(80.0),
                acc: 5.0,
            }),
            accel: Some(Accel {
                x: Some(1.0),
                y: None,
                z: Some(3.0),
            }),
        };
        assert_eq!(roundtrip(&sample), sample);
    }

    /// The wire shape the frontend actually sends: a string uuid, a millis
    /// timestamp, and just the one source present.
    #[test]
    fn deserializes_frontend_gps_shape() {
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000004",
            "t": 1700000000003,
            "gps": { "lat": 55.95, "lon": -3.19, "alt": null, "acc": 8.5 }
        }"#;
        let sample: Sample = serde_json::from_str(json).expect("deserialize");
        assert_eq!(sample.id, Uuid::from_u128(4));
        assert_eq!(sample.t, 1_700_000_000_003);
        assert!(sample.accel.is_none());
        assert_eq!(sample.gps.expect("gps present").lat, 55.95);
    }

    /// A gps-only sample must not carry an `accel` key on the wire (and vice-versa).
    #[test]
    fn omits_absent_source_from_json() {
        let sample = Sample {
            id: Uuid::from_u128(5),
            t: 1_700_000_000_004,
            gps: None,
            accel: Some(Accel {
                x: Some(0.0),
                y: Some(0.0),
                z: Some(0.0),
            }),
        };
        let json = serde_json::to_string(&sample).expect("serialize");
        assert!(!json.contains("gps"), "gps key should be omitted: {json}");
        assert!(json.contains("accel"));
    }
}
