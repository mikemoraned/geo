use geo::Area;
use geo_overturemaps::{context::{ClippingMode, OvertureContext}, model::GersId};

#[tokio::test]
async fn test_find_geometry_by_id() {
    let release_path = "tests/data/release/2025-06-25.0";
    let om = OvertureContext::load_from_release(release_path).await.unwrap();

    let amsterdam_id = GersId::new("dbd84987-2831-4b62-a0e0-a3f3d5a237c2".to_string());
    let geometry = om.find_geometry_by_id(&amsterdam_id).await.unwrap();
    assert!(geometry.is_some(), "Expected geometry for Amsterdam ID to be found");
}

#[tokio::test]
async fn test_find_land_cover_in_region_clip_to_region() {
    let release_path = "tests/data/release/2025-06-25.0";
    let om = OvertureContext::load_from_release(release_path).await.unwrap();

    let amsterdam_id = GersId::new("dbd84987-2831-4b62-a0e0-a3f3d5a237c2".to_string());
    let geometry = om.find_geometry_by_id(&amsterdam_id).await.unwrap().unwrap();

    let land_cover = om.find_land_cover_in_region(&geometry, ClippingMode::ClipToRegion).await.unwrap().unwrap();
    assert!(land_cover.signed_area() > 0.0, "Expected land cover area to be greater than zero");
}