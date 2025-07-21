use geo_overturemaps::{context::OvertureContext, io::save_as_geojson, model::GersId};

#[tokio::test]
async fn test_save_geometry_as_geojson() {
    let release_path = "tests/data/release/2025-06-25.0";
    let om = OvertureContext::load_from_release(release_path).await.unwrap();

    let amsterdam_id = GersId::new("dbd84987-2831-4b62-a0e0-a3f3d5a237c2".to_string());
    let geometry = om.find_geometry_by_id(&amsterdam_id).await.unwrap().unwrap();

    let output_path = "tests/out/test_save_geometry_as_geojson.geojson";
    save_as_geojson(&geometry, output_path).unwrap();
    assert!(std::path::Path::new(output_path).exists(), "GeoJSON file should be created");
}