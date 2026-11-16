use std::sync::Arc;

use arrow::array::{ArrayRef, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use chrono::{TimeZone, Utc};
use domain::{CrossingCompactId, CrossingId};
use geo_types::{Geometry, LineString, Point, polygon};
use map::base::{BaseError, write_base_map};
use medallion::{
    COUNTRY, Country, GEOMETRY, PROJECTED_GEOMETRY, Projector, Root, geo_batch,
    projected_wkb_field, wkb_column, wkb_field,
};
use medallion_model::{OVERTURE_EXTRACT, OverlapKind, WATER_CROSSING_V2, WaterCrossingRowV2};
use serde_json::{Value, json};

const EXTRACT: &str = "20261011T154538Z";
const OTHER_EXTRACT: &str = "20260727T193628Z";

fn crossing(n: u32, rail: &str, water: &str, extract: &str) -> WaterCrossingRowV2 {
    WaterCrossingRowV2 {
        crossing_id: CrossingId::new(format!("{water}:{rail}@{n}")).unwrap(),
        crossing_compact_id: CrossingCompactId::new(0x1000_0000 + n),
        overture_water_id: water.into(),
        overture_water_subtype: Some("river".into()),
        overture_water_class: Some("river".into()),
        track_id: "track".into(),
        overture_rail_id: rail.into(),
        overture_rail_class: Some("standard_gauge".into()),
        overlap_kind: OverlapKind::Point,
        overlap_m: 0.0,
        total_overlap_m: 0.0,
        merged_parts: 1,
        frac: 0.5,
        extract_id: extract.into(),
        merge_distance_m: 100.0,
        min_crossing_m: 5.0,
    }
}

async fn store_crossings(root: &Root, rows: &[WaterCrossingRowV2]) {
    let country = Country::Germany;
    let projector = Projector::for_country(country).unwrap();
    let points: Vec<Point<f64>> = rows.iter().map(|_| Point::new(13.4, 52.5)).collect();
    let projected: Vec<Point<f64>> = points
        .iter()
        .map(|point| projector.project(point).unwrap())
        .collect();
    let batch = geo_batch(
        rows,
        &[
            (wkb_field(GEOMETRY).unwrap(), points.as_slice()),
            (
                projected_wkb_field(PROJECTED_GEOMETRY, country).unwrap(),
                projected.as_slice(),
            ),
        ],
    )
    .unwrap();
    root.dataset(WATER_CROSSING_V2)
        .partition(COUNTRY, country)
        .unwrap()
        .replace_with_geo(&[batch])
        .await
        .unwrap();
}

struct Extracted {
    id: &'static str,
    subtype: Option<&'static str>,
    class: &'static str,
    geometry: Geometry<f64>,
}

async fn store_extract(
    root: &Root,
    extract: &str,
    theme: &str,
    of_type: &str,
    features: &[Extracted],
) {
    let strings = |values: Vec<Option<&str>>| -> ArrayRef { Arc::new(StringArray::from(values)) };
    let (geometry_field, geometry) = wkb_column(
        Field::new(GEOMETRY, DataType::Binary, true),
        features.iter().map(|feature| &feature.geometry),
    )
    .unwrap();
    let batch = RecordBatch::try_new(
        Arc::new(Schema::new(vec![
            Arc::new(Field::new("id", DataType::Utf8, true)),
            Arc::new(Field::new("subtype", DataType::Utf8, true)),
            Arc::new(Field::new("class", DataType::Utf8, true)),
            geometry_field,
            Arc::new(Field::new("extract_id", DataType::Utf8, true)),
        ])),
        vec![
            strings(features.iter().map(|feature| Some(feature.id)).collect()),
            strings(features.iter().map(|feature| feature.subtype).collect()),
            strings(features.iter().map(|feature| Some(feature.class)).collect()),
            geometry,
            strings(features.iter().map(|_| Some(extract)).collect()),
        ],
    )
    .unwrap();
    root.dataset(OVERTURE_EXTRACT)
        .for_id(extract)
        .unwrap()
        .partition("theme", theme)
        .unwrap()
        .partition("type", of_type)
        .unwrap()
        .append(
            Utc.with_ymd_and_hms(2026, 10, 11, 15, 45, 38).unwrap(),
            &[batch],
        )
        .await
        .unwrap();
}

fn rail(id: &'static str, lon: f64) -> Extracted {
    Extracted {
        id,
        subtype: Some("rail"),
        class: "standard_gauge",
        geometry: Geometry::LineString(LineString::from(vec![(lon, 52.5), (lon + 0.1, 52.5)])),
    }
}

fn water(id: &'static str, lon: f64) -> Extracted {
    Extracted {
        id,
        subtype: Some("river"),
        class: "river",
        geometry: Geometry::Polygon(polygon![
            (x: lon, y: 52.4),
            (x: lon + 0.1, y: 52.4),
            (x: lon + 0.1, y: 52.6),
            (x: lon, y: 52.4),
        ]),
    }
}

async fn store_with_extracts(root: &Root) {
    store_extract(
        root,
        EXTRACT,
        "transportation",
        "segment",
        &[
            rail("rail-a", 13.0),
            rail("rail-b", 14.0),
            rail("rail-unnamed", 15.0),
        ],
    )
    .await;
    store_extract(
        root,
        OTHER_EXTRACT,
        "transportation",
        "segment",
        &[rail("rail-a", 9.0)],
    )
    .await;
    store_extract(
        root,
        EXTRACT,
        "base",
        "water",
        &[water("water-a", 13.0), water("water-unnamed", 15.0)],
    )
    .await;
}

fn lines(bytes: Vec<u8>) -> Vec<Value> {
    String::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test]
async fn each_named_feature_is_drawn_once_from_its_crossings_own_extract() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    store_crossings(
        &root,
        &[
            crossing(1, "rail-a", "water-a", EXTRACT),
            crossing(2, "rail-a", "water-a", EXTRACT),
            crossing(3, "rail-b", "water-a", EXTRACT),
        ],
    )
    .await;
    store_with_extracts(&root).await;

    let mut bytes = Vec::new();
    let written = write_base_map(&root, &mut bytes).await.unwrap();
    let mut features = lines(bytes);
    features.sort_by_key(|feature| feature["geometry"]["coordinates"].to_string());

    assert_eq!((written.rail, written.water), (2, 1));
    assert_eq!(
        features,
        [
            json!({
                "type": "Feature",
                "tippecanoe": { "layer": "rail", "minzoom": 7 },
                "geometry": { "type": "LineString", "coordinates": [[13.0, 52.5], [13.1, 52.5]] },
                "properties": { "class": "standard_gauge" }
            }),
            json!({
                "type": "Feature",
                "tippecanoe": { "layer": "rail", "minzoom": 7 },
                "geometry": { "type": "LineString", "coordinates": [[14.0, 52.5], [14.1, 52.5]] },
                "properties": { "class": "standard_gauge" }
            }),
            json!({
                "type": "Feature",
                "tippecanoe": { "layer": "water", "minzoom": 7 },
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[[13.0, 52.4], [13.1, 52.4], [13.1, 52.6], [13.0, 52.4]]]
                },
                "properties": { "subtype": "river", "class": "river" }
            }),
        ]
    );
}

#[tokio::test]
async fn a_feature_missing_from_its_crossings_extract_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    store_crossings(
        &root,
        &[
            crossing(1, "rail-a", "water-a", EXTRACT),
            crossing(2, "rail-gone", "water-a", EXTRACT),
        ],
    )
    .await;
    store_with_extracts(&root).await;

    let err = write_base_map(&root, Vec::new()).await.unwrap_err();

    assert!(
        matches!(
            err,
            BaseError::Unfound {
                layer: "rail",
                country: Country::Germany,
                named: 2,
                found: 1,
            }
        ),
        "{err}"
    );
}
