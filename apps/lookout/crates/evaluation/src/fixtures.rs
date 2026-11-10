#![cfg(any(test, feature = "fixtures"))]

use std::num::NonZeroUsize;
use std::path::Path;

use chrono::{DateTime, TimeDelta, Utc};
use domain::places::DRESDEN_HBF;
use domain::{CrossingCompactId, CrossingId, Percentage, SessionId, Span};
use geo_types::{LineString, Point};
use medallion::{
    Country, GeographyRow, Root, Row, write_geography_rows, write_geography_rows_by_country,
    write_rows,
};
use medallion_model::{
    FloatType, TrainSessionCrossingRowV1, TrainSessionRowV3, TrainSessionSampleRowV3,
    WaterCrossingRowV2,
};

use crate::pick::{Pick, Seed};
use crate::score::{Score, Scoring};
use crate::settings::OracleSettings;
use crate::steps::{Predictor, Run};
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
    let rows: Vec<GeographyRow<WaterCrossingRowV2, Point<f64>>> = placed
        .iter()
        .map(|&(n, lat, lon)| GeographyRow {
            row: WaterCrossingRowV2 {
                crossing_id: crossing(n),
                crossing_compact_id: CrossingCompactId::new(n),
                ..WaterCrossingRowV2::example()
            },
            geography: Point::new(lon, lat),
            country: Country::Germany,
        })
        .collect();
    write_geography_rows_by_country(root, &rows)
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
    Predictor::Oracle(OracleSettings {
        horizon: Span::new("5m").expect("a span"),
    })
}
