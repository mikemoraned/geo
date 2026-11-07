#![cfg(any(test, feature = "fixtures"))]

use std::num::NonZeroUsize;
use std::path::Path;

use chrono::{DateTime, TimeDelta, Utc};
use domain::places::DRESDEN_HBF;
use domain::{CrossingCompactId, CrossingId, Percentage, SessionId, Span};
use geo_types::{LineString, Point};
use medallion::{
    COUNTRY, Country, GEOMETRY, GeographyRow, PROJECTED_GEOMETRY, Projector, Root, Row, geo_batch,
    projected_wkb_field, wkb_field, write_geography_rows, write_rows,
};
use medallion_model::{
    TrainSessionCrossingRowV1, TrainSessionRowV3, TrainSessionSampleRowV3, WATER_CROSSING_V2,
    WaterCrossingRowV2,
};

use crate::pick::{Pick, Seed};
use crate::score::{Score, Scoring};
use crate::steps::{FloatType, Predictor, Run};
use crate::summarise::Bootstrap;

pub fn instant(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(1_785_098_609_000).expect("an instant")
        + TimeDelta::seconds(seconds)
}

pub fn session(n: usize) -> SessionId {
    SessionId::new(format!("session-{n}")).expect("a session id")
}

pub fn crossing(n: u32) -> CrossingId {
    CrossingId::new(format!("crossing-{n}")).expect("a crossing id")
}

pub async fn write_crossings(root: &Root, placed: &[(u32, f64, f64)]) {
    let projector = Projector::for_country(Country::Germany).expect("a projector");
    let rows: Vec<WaterCrossingRowV2> = placed
        .iter()
        .map(|&(n, _, _)| WaterCrossingRowV2 {
            crossing_id: crossing(n),
            crossing_compact_id: CrossingCompactId::new(n),
            ..WaterCrossingRowV2::example()
        })
        .collect();
    let points: Vec<Point<f64>> = placed
        .iter()
        .map(|&(_, lat, lon)| Point::new(lon, lat))
        .collect();
    let projected: Vec<Point<f64>> = points
        .iter()
        .map(|point| projector.project(point).expect("a projection"))
        .collect();
    let batch = geo_batch(
        &rows,
        &[
            (wkb_field(GEOMETRY).expect("a field"), points.as_slice()),
            (
                projected_wkb_field(PROJECTED_GEOMETRY, Country::Germany).expect("a field"),
                projected.as_slice(),
            ),
        ],
    )
    .expect("a batch");
    root.dataset(WATER_CROSSING_V2)
        .partition(COUNTRY, Country::Germany)
        .expect("a partition")
        .replace_with_geo(&[batch])
        .await
        .expect("write the crossings");
}

pub async fn store(dir: &Path, sessions: usize) -> Root {
    let root = Root::new(dir);
    let (lon, lat) = DRESDEN_HBF.x_y();
    let line = LineString::from(vec![(lon, lat), (lon, lat + 0.01)]);
    let session_rows: Vec<GeographyRow<TrainSessionRowV3, LineString<f64>>> = (0..sessions)
        .map(|n| GeographyRow {
            row: TrainSessionRowV3 {
                session_id: session(n),
                started_at: instant(0),
                ended_at: instant(600),
                realtime_percentage: Percentage::new(100.0).expect("a percentage"),
                ..TrainSessionRowV3::example()
            },
            geography: line.clone(),
            country: Country::Germany,
        })
        .collect();
    write_geography_rows(&root, &session_rows)
        .await
        .expect("write the sessions");
    let samples: Vec<GeographyRow<TrainSessionSampleRowV3, Point<f64>>> = (0..sessions)
        .flat_map(|n| {
            (0..=60).map(move |step| GeographyRow {
                row: TrainSessionSampleRowV3 {
                    session_id: session(n),
                    t: instant(step * 10),
                    seq: step as u32,
                    lat,
                    lon,
                    ..TrainSessionSampleRowV3::example()
                },
                geography: DRESDEN_HBF,
                country: Country::Germany,
            })
        })
        .collect();
    write_geography_rows(&root, &samples)
        .await
        .expect("write the samples");
    let passes: Vec<TrainSessionCrossingRowV1> = (0..sessions)
        .map(|n| TrainSessionCrossingRowV1 {
            session_id: session(n),
            crossing_id: crossing(1),
            crossed_at: instant(300),
            ..TrainSessionCrossingRowV1::example()
        })
        .collect();
    write_rows(&root, &passes).await.expect("write the passes");
    write_crossings(&root, &[(1, lat + 0.005, lon), (2, lat + 0.5, lon)]).await;
    root
}

pub fn first_runs() -> Scoring {
    Scoring {
        horizon: Span::new("5m").expect("a span"),
        last_moment: Span::new("30s").expect("a span"),
        ramp: Span::new("1m").expect("a span"),
        last_moment_score: Score::new(0.25).expect("a score"),
        floor: Score::new(0.05).expect("a score"),
        warm_up: Span::new("1m").expect("a span"),
    }
}

pub fn run_of(predictor: Predictor) -> Run {
    Run {
        pick: Pick {
            country: Country::Germany,
            realtime_at_least: Percentage::new(90.0).expect("a percentage"),
            sessions: NonZeroUsize::new(3).expect("not zero"),
            seed: Seed(1),
        },
        predictor,
        float_type: FloatType::F32,
        scoring: first_runs(),
        bootstrap: Bootstrap {
            resamples: NonZeroUsize::new(100).expect("not zero"),
            seed: Seed(1),
        },
    }
}

pub fn oracle() -> Predictor {
    Predictor::Oracle {
        horizon: Span::new("5m").expect("a span"),
    }
}
