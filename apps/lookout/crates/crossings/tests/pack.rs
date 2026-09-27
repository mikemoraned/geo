use std::collections::HashMap;

use crossings::{pointset, silver};
use domain::{CrossingCompactId, CrossingId};
use geo_types::Point as GeoPoint;
use medallion::{
    COUNTRY, Country, GEOMETRY, PROJECTED_GEOMETRY, Projector, Root, geo_batch,
    projected_wkb_field, wkb_field,
};
use medallion_model::{OverlapKind, WaterCrossingRow};

fn compact_id(n: usize) -> CrossingCompactId {
    CrossingCompactId::new(0x1000_0000 + n as u32)
}

const LON: f64 = 13.548209;
const LAT: f64 = 51.617567;

const EXTRACT: &str = "20260727T193628Z";

async fn store_with_crossings(root: &Root, country: &str, positions: &[(f64, f64)]) {
    let projector = Projector::for_country(Country::Germany).expect("projector");
    let rows: Vec<WaterCrossingRow> = positions
        .iter()
        .enumerate()
        .map(|(n, _)| WaterCrossingRow {
            crossing_id: CrossingId::new(format!("water:track:rail@{n}")).expect("id"),
            crossing_compact_id: compact_id(n),
            water_id: "water".into(),
            water_subtype: Some("river".into()),
            water_class: Some("river".into()),
            track_id: "track".into(),
            rail_id: format!("rail-{n}"),
            rail_class: Some("rail".into()),
            overlap_kind: OverlapKind::Point,
            overlap_m: 0.0,
            total_overlap_m: 0.0,
            merged_parts: 1,
            frac: n as f64 / 10.0,
            extract_id: EXTRACT.into(),
            merge_distance_m: 100.0,
            min_crossing_m: 5.0,
        })
        .collect();
    let points: Vec<GeoPoint<f64>> = positions
        .iter()
        .map(|(lon, lat)| GeoPoint::new(*lon, *lat))
        .collect();
    let projected: Vec<GeoPoint<f64>> = points
        .iter()
        .map(|point| projector.project(point).expect("project"))
        .collect();

    let batch = geo_batch(
        &rows,
        &[
            (wkb_field(GEOMETRY).expect("field"), points.as_slice()),
            (
                projected_wkb_field(PROJECTED_GEOMETRY, Country::Germany).expect("field"),
                projected.as_slice(),
            ),
        ],
    )
    .expect("build the batch");

    root.dataset(medallion_model::WATER_CROSSING)
        .partition(COUNTRY, country)
        .expect("partition")
        .replace_with_geo(&[batch])
        .await
        .expect("write the crossings");
}

#[tokio::test]
async fn a_crossing_is_read_with_its_position_and_the_name_the_store_gave_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    store_with_crossings(&root, "DE", &[(LON, LAT)]).await;

    let crossings = silver::read(&root).await.unwrap();

    assert_eq!(crossings.len(), 1);
    let crossing = &crossings[0];
    assert_eq!(crossing.crossing.id.to_string(), "water:track:rail@0");
    assert_eq!(crossing.compact_id, compact_id(0));
    assert_eq!(crossing.extract_id, EXTRACT);
    assert!((crossing.crossing.longitude() - LON).abs() < 1e-9);
    assert!((crossing.crossing.latitude() - LAT).abs() < 1e-9);
}

#[tokio::test]
async fn every_country_the_store_holds_is_packed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    store_with_crossings(&root, "DE", &[(LON, LAT)]).await;
    store_with_crossings(&root, "FR", &[(LON + 0.01, LAT), (LON + 0.02, LAT)]).await;

    let crossings = silver::read(&root).await.unwrap();

    assert_eq!(crossings.len(), 3);
}

#[tokio::test]
async fn a_store_without_the_dataset_says_which_one_is_missing() {
    let tmp = tempfile::tempdir().unwrap();

    let err = silver::read(&Root::new(tmp.path())).await.unwrap_err();

    assert!(matches!(
        err,
        silver::ReadError::Missing {
            dataset: "water_crossing"
        }
    ));
}

#[tokio::test]
async fn what_the_store_holds_survives_being_packed_and_read_back() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    store_with_crossings(&root, "DE", &[(LON, LAT), (LON + 0.01, LAT + 0.01)]).await;

    let crossings = silver::read(&root).await.unwrap();
    let unpacked = pointset::unpack(&packed(&crossings)).unwrap();

    assert_eq!(unpacked.len(), crossings.len());
    for crossing in &crossings {
        let point = unpacked
            .iter()
            .find(|point| point.longitude() == crossing.crossing.longitude() as f32)
            .expect("the crossing is in the buffer");
        assert_eq!(point.latitude(), crossing.crossing.latitude() as f32);
    }
}

#[tokio::test]
async fn every_packed_id_maps_back_to_exactly_one_crossing_the_store_named() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    let positions: Vec<(f64, f64)> = (0..50)
        .map(|n| (LON + n as f64 * 0.001, LAT + n as f64 * 0.001))
        .collect();
    store_with_crossings(&root, "DE", &positions).await;

    let crossings = silver::read(&root).await.unwrap();
    let unpacked = pointset::unpack(&packed(&crossings)).unwrap();

    let by_id: HashMap<CrossingCompactId, &CrossingId> = crossings
        .iter()
        .map(|crossing| (crossing.compact_id, &crossing.crossing.id))
        .collect();
    assert_eq!(by_id.len(), crossings.len(), "ids are distinct");
    for point in &unpacked {
        assert!(
            by_id.contains_key(&point.id),
            "the packed id {} names a crossing the store holds",
            point.id
        );
    }
}

fn packed(crossings: &[silver::Crossing]) -> Vec<u8> {
    let points: Vec<_> = crossings
        .iter()
        .map(crossings::compacted)
        .collect::<Result<Vec<_>, _>>()
        .expect("on the globe");

    pointset::pack(&points).unwrap()
}
