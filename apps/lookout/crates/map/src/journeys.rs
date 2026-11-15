use std::io::Write;

use chrono::{DateTime, Utc};
use domain::{Gps, SessionId};
use geo::BoundingRect;
use geo_types::LineString;
use geojson::{Geometry, GeometryValue};
use serde::Serialize;
use session_crossings::gold::Replay;

use crate::feature::{Feature, FeatureError, Tippecanoe};

pub const SAMPLES_ZOOM: u8 = 6;
pub const ROUTE_MAX_ZOOM: u8 = 12;

#[derive(Debug, thiserror::Error)]
pub enum JourneyError {
    #[error("session {session} has {samples} samples, and a route needs at least two")]
    TooFewSamples { session: SessionId, samples: usize },
    #[error(transparent)]
    Feature(#[from] FeatureError),
}

const ROUTE: Tippecanoe = Tippecanoe {
    layer: "route",
    minzoom: 0,
    maxzoom: Some(ROUTE_MAX_ZOOM),
};

const SAMPLES: Tippecanoe = Tippecanoe {
    layer: "samples",
    minzoom: SAMPLES_ZOOM,
    maxzoom: Some(SAMPLES_ZOOM),
};

#[derive(Debug, Serialize)]
struct RouteProperties<'a> {
    session: &'a SessionId,
    crossings: usize,
    samples_zoom: u8,
    min_lon: f64,
    min_lat: f64,
    max_lon: f64,
    max_lat: f64,
}

#[derive(Debug, Serialize)]
struct SampleProperties<'a> {
    session: &'a SessionId,
    seq: usize,
    t: DateTime<Utc>,
    #[serde(flatten)]
    gps: Gps<f64>,
}

pub fn write_journeys(replays: &[Replay], mut to: impl Write) -> Result<usize, JourneyError> {
    let mut written = 0;
    for replay in replays {
        route(replay)?.write_line(&mut to)?;
        for (seq, sample) in replay.samples.iter().enumerate() {
            Feature {
                tippecanoe: SAMPLES,
                geometry: Geometry::new(GeometryValue::from(&sample.gps.position)),
                properties: SampleProperties {
                    session: &replay.session,
                    seq,
                    t: sample.t,
                    gps: sample.gps,
                },
            }
            .write_line(&mut to)?;
        }
        written += 1 + replay.samples.len();
    }
    Ok(written)
}

fn route(replay: &Replay) -> Result<Feature<RouteProperties<'_>>, JourneyError> {
    let line: LineString<f64> = replay
        .samples
        .iter()
        .map(|sample| sample.gps.position)
        .collect();
    let too_few = || JourneyError::TooFewSamples {
        session: replay.session.clone(),
        samples: replay.samples.len(),
    };
    if line.0.len() < 2 {
        return Err(too_few());
    }
    let bounds = line.bounding_rect().ok_or_else(too_few)?;

    Ok(Feature {
        tippecanoe: ROUTE,
        geometry: Geometry::new(GeometryValue::from(&line)),
        properties: RouteProperties {
            session: &replay.session,
            crossings: replay.crossings,
            samples_zoom: SAMPLES_ZOOM,
            min_lon: bounds.min().x,
            min_lat: bounds.min().y,
            max_lon: bounds.max().x,
            max_lat: bounds.max().y,
        },
    })
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use domain::Sample;
    use serde_json::{Value, json};

    use super::*;

    fn replay(samples: Vec<Sample<f64>>) -> Replay {
        Replay {
            session: SessionId::new("a489b094-ff85-5c4d-8de7-8cf7cf753ec1").unwrap(),
            crossings: 43,
            samples,
        }
    }

    fn sample(second: u32, lat: f64, lon: f64) -> Sample<f64> {
        Sample::at(Utc.with_ymd_and_hms(2026, 7, 22, 14, 28, second).unwrap(), lat, lon).unwrap()
    }

    fn written(replays: &[Replay]) -> Vec<Value> {
        let mut bytes = Vec::new();
        write_journeys(replays, &mut bytes).unwrap();
        String::from_utf8(bytes)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn a_session_is_one_route_line_then_a_point_per_sample() {
        let lines = written(&[replay(vec![
            sample(59, 49.479479, 8.468973)
                .with_altitude_metres(Some(99.0))
                .with_accuracy_metres(Some(84.8)),
            sample(0, 49.479518, 8.469109).with_speed_mps(Some(1.5)),
        ])]);

        assert_eq!(
            lines,
            [
                json!({
                    "type": "Feature",
                    "tippecanoe": { "layer": "route", "minzoom": 0, "maxzoom": 12 },
                    "geometry": {
                        "type": "LineString",
                        "coordinates": [[8.468973, 49.479479], [8.469109, 49.479518]]
                    },
                    "properties": {
                        "session": "a489b094-ff85-5c4d-8de7-8cf7cf753ec1",
                        "crossings": 43,
                        "samples_zoom": 6,
                        "min_lon": 8.468973,
                        "min_lat": 49.479479,
                        "max_lon": 8.469109,
                        "max_lat": 49.479518
                    }
                }),
                json!({
                    "type": "Feature",
                    "tippecanoe": { "layer": "samples", "minzoom": 6, "maxzoom": 6 },
                    "geometry": { "type": "Point", "coordinates": [8.468973, 49.479479] },
                    "properties": {
                        "session": "a489b094-ff85-5c4d-8de7-8cf7cf753ec1",
                        "seq": 0,
                        "t": "2026-07-22T14:28:59Z",
                        "lat": 49.479479,
                        "lon": 8.468973,
                        "alt": 99.0,
                        "acc": 84.8,
                        "speed": null,
                        "heading": null
                    }
                }),
                json!({
                    "type": "Feature",
                    "tippecanoe": { "layer": "samples", "minzoom": 6, "maxzoom": 6 },
                    "geometry": { "type": "Point", "coordinates": [8.469109, 49.479518] },
                    "properties": {
                        "session": "a489b094-ff85-5c4d-8de7-8cf7cf753ec1",
                        "seq": 1,
                        "t": "2026-07-22T14:28:00Z",
                        "lat": 49.479518,
                        "lon": 8.469109,
                        "alt": null,
                        "acc": null,
                        "speed": 1.5,
                        "heading": null
                    }
                }),
            ]
        );
    }

    #[test]
    fn a_session_of_one_sample_has_no_route() {
        let mut bytes = Vec::new();

        let err = write_journeys(&[replay(vec![sample(0, 49.0, 8.0)])], &mut bytes).unwrap_err();

        assert!(
            matches!(err, JourneyError::TooFewSamples { samples: 1, .. }),
            "{err}"
        );
    }
}
