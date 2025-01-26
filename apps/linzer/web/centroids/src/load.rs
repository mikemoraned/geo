
use geo_types::{Geometry, GeometryCollection};
use tracing::{info, warn};

pub async fn fetch_text(source_url: &String) -> Result<String, Box<dyn std::error::Error>> {
    info!("Fetching text from '{source_url}' ...");

    let response = reqwest::get(source_url).await?;
    match response.status() {
        reqwest::StatusCode::OK => {
            info!("Response status: OK");

            let text = response.text().await?;
            info!("Fetched text");
            Ok(text)
        },
        status => {
            warn!("Response status: NOT OK: {status}");
            Err("failed to fetch geojson".into())
        }
    }
}

pub fn parse_geojson_to_geometry_collection(text: String) -> Result<GeometryCollection<f64>, Box<dyn std::error::Error>> {
    use geozero::geojson::*;
    use geozero::ToGeo;

    let geojson = GeoJsonString(text);
    if let Ok(Geometry::GeometryCollection(collection)) = geojson.to_geo() {
        info!("Extracted geometries");
        Ok(collection.clone())
    }
    else {
        warn!("Failed to extract geometries");
        Err("failed to extract geometries".into())
    }
}
