//! Packing what the store actually holds.
//!
//! The buffer's layout is checked in the unit tests; what cannot be checked there is the part
//! between the store and them — reading the crossings out of GeoParquet files written by the
//! same code that writes the real ones, and taking a position out of the geometry column.

use crossings::{Point, id, pointset, silver};
use geo_types::Point as GeoPoint;
use medallion::{
    COUNTRY, Country, GEOMETRY, PROJECTED_GEOMETRY, Projector, Root, geo_batch,
    projected_wkb_field, wkb_field,
};
use model::{CrossingId, OverlapKind, WaterCrossingRow};

/// Ruhland, where a line crosses the Schwarze Elster.
const LON: f64 = 13.548209;
const LAT: f64 = 51.617567;

const EXTRACT: &str = "20260727T193628Z";

/// Write `positions` as the crossings of one country, the way the crossings pipeline writes
/// them: one file per country, both geometries, as GeoParquet.
///
/// `country` is the partition value rather than a [`Country`], so a test can write a store
/// holding more countries than the code knows how to project into.
async fn store_with_crossings(root: &Root, country: &str, positions: &[(f64, f64)]) {
    let projector = Projector::for_country(Country::Germany).expect("projector");
    let rows: Vec<WaterCrossingRow> = positions
        .iter()
        .enumerate()
        .map(|(n, _)| WaterCrossingRow {
            // The position is not among these columns: it is the geometry below.
            crossing_id: CrossingId::new(format!("water:track:rail@{n}")).expect("id"),
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

    root.dataset(model::WATER_CROSSING)
        .partition(COUNTRY, country)
        .expect("partition")
        .replace_with_geo(&[batch])
        .await
        .expect("write the crossings");
}

#[tokio::test]
async fn a_crossing_is_read_with_its_position_and_its_silver_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    store_with_crossings(&root, "DE", &[(LON, LAT)]).await;

    let crossings = silver::read(&root).await.unwrap();

    assert_eq!(crossings.len(), 1);
    let crossing = &crossings[0];
    assert_eq!(crossing.crossing_id.to_string(), "water:track:rail@0");
    assert_eq!(crossing.extract_id, EXTRACT);
    assert!((crossing.position.x - LON).abs() < 1e-9);
    assert!((crossing.position.y - LAT).abs() < 1e-9);
}

/// The device is switched on wherever its owner takes it, and the buffer's coordinates are
/// lat/lon, so a run packs the whole store rather than a country of it.
#[tokio::test]
async fn every_country_the_store_holds_is_packed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    store_with_crossings(&root, "DE", &[(LON, LAT)]).await;
    store_with_crossings(&root, "FR", &[(LON + 0.01, LAT), (LON + 0.02, LAT)]).await;

    let crossings = silver::read(&root).await.unwrap();

    assert_eq!(crossings.len(), 3);
}

/// Packing before the dataset exists is a run out of order, not an empty buffer to ship.
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

/// What the device ends up holding, from the store to the packed bytes and back.
#[tokio::test]
async fn what_the_store_holds_survives_being_packed_and_read_back() {
    let tmp = tempfile::tempdir().unwrap();
    let root = Root::new(tmp.path());
    store_with_crossings(&root, "DE", &[(LON, LAT), (LON + 0.01, LAT + 0.01)]).await;

    let crossings = silver::read(&root).await.unwrap();
    let ids = id::assign(&crossings).unwrap();
    let points: Vec<Point> = crossings
        .iter()
        .zip(&ids)
        .map(|(crossing, id)| Point::of(crossing, *id))
        .collect();

    let unpacked = pointset::unpack(&pointset::pack(&points).unwrap()).unwrap();

    assert_eq!(unpacked.len(), crossings.len());
    for crossing in &crossings {
        let point = unpacked
            .iter()
            .find(|point| point.longitude == crossing.position.x as f32)
            .expect("the crossing is in the buffer");
        assert_eq!(point.latitude, crossing.position.y as f32);
    }
}
